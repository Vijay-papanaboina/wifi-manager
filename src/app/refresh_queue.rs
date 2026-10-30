//! Coalesce refresh requests without ever blocking a D-Bus signal consumer.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::task::{Poll, Waker};
use std::time::Duration;

use futures_util::{Stream, StreamExt, future::poll_fn};
use gtk4::glib;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum RefreshKind {
    Networks,
    Device,
}

/// One pending request, with device-state work taking precedence over an AP
/// update. Notifications during a refresh guarantee a follow-up refresh.
#[derive(Default)]
pub(super) struct RefreshQueue {
    pending: Cell<Option<RefreshKind>>,
    waiter: RefCell<Option<Waker>>,
}

impl RefreshQueue {
    pub(super) fn request(&self, kind: RefreshKind) {
        self.pending.set(Some(self.pending.get().map_or(kind, |pending| pending.max(kind))));
        if let Some(waiter) = self.waiter.borrow_mut().take() {
            waiter.wake();
        }
    }

    async fn next(&self) -> RefreshKind {
        poll_fn(|cx| {
            if let Some(kind) = self.pending.take() {
                Poll::Ready(kind)
            } else {
                *self.waiter.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }

    /// Run exactly one worker per queue. The fixed coalescing window does not
    /// restart on each event, so sustained churn cannot starve UI updates.
    pub(super) async fn run<F, Fut>(&self, delay: Duration, mut refresh: F)
    where
        F: FnMut(RefreshKind) -> Fut,
        Fut: Future<Output = ()>,
    {
        loop {
            let mut kind = self.next().await;
            glib::timeout_future(delay).await;
            if let Some(pending) = self.pending.take() {
                kind = kind.max(pending);
            }
            refresh(kind).await;
        }
    }
}

/// The callback must only do synchronous bookkeeping and enqueue work. Waiting
/// for D-Bus here can fill the signal queue and prevent that very reply from
/// being read (zbus issue #213).
pub(super) async fn drain_signals<S, F>(signals: S, mut on_signal: F)
where
    S: Stream,
    F: FnMut(S::Item),
{
    futures_util::pin_mut!(signals);
    while let Some(signal) = signals.next().await {
        on_signal(signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::{Either, select};

    use crate::dbus::network_manager::WifiManager;
    use crate::dbus::test_support::{
        FakeAccessPointConfig, FakeNetworkManager, TestResult, WifiSignal,
    };

    fn run_burst_test(signal: WifiSignal) -> TestResult<()> {
        let context = glib::MainContext::new();
        context.with_thread_default(|| {
            context.block_on(async {
                let scenario = async {
                    let fake = FakeNetworkManager::start().await?;
                    let old = FakeAccessPointConfig::new(1, "Kirjasto");
                    let new = FakeAccessPointConfig::new(2, "ePhone");
                    let old_path = old.path.clone();
                    let new_path = new.path.clone();
                    fake.add_access_point(old).await?;
                    fake.add_access_point(new).await?;
                    fake.set_visible_access_points(vec![old_path]);
                    let wifi = WifiManager::from_connection(fake.client_connection()).await?;
                    let signals = fake.signal_stream(signal).await?;
                    fake.burst_on_next_enumeration(signal, 128, vec![new_path]);

                    let requests = RefreshQueue::default();
                    let completed = RefreshQueue::default();
                    let snapshots = RefCell::new(Vec::new());
                    let kind = match signal {
                        WifiSignal::StateChanged => RefreshKind::Device,
                        _ => RefreshKind::Networks,
                    };
                    // Exercise the SAME consumer and worker used by live_updates,
                    // including a real D-Bus reply behind a >64-signal burst.
                    let consumer = drain_signals(signals, |message| {
                        message.expect("valid signal");
                        requests.request(kind);
                    });
                    let worker = requests.run(Duration::from_millis(1), |kind| {
                        let wifi = &wifi;
                        let snapshots = &snapshots;
                        let completed = &completed;
                        async move {
                            let networks = wifi.get_networks().await.expect("refresh completes");
                            snapshots.borrow_mut().push((kind, networks));
                            completed.request(kind);
                        }
                    });

                    fake.emit_signal(signal).await?;
                    let observe = async {
                        completed.next().await;
                        assert_eq!(snapshots.borrow()[0].1[0].ssid, "Kirjasto");
                        completed.next().await;
                        {
                            let snapshots = snapshots.borrow();
                            assert_eq!(
                                snapshots.len(),
                                2,
                                "the burst must coalesce into one follow-up"
                            );
                            assert_eq!(snapshots[1].0, kind);
                            assert_eq!(snapshots[1].1.len(), 1);
                            assert_eq!(snapshots[1].1[0].ssid, "ePhone");
                        }

                        // A subsequent manual scan/enumeration on this same
                        // connection must still work after the burst.
                        wifi.request_scan().await?;
                        assert_eq!(wifi.get_networks().await?[0].ssid, "ePhone");
                        assert_eq!(snapshots.borrow().len(), 2);
                        Ok(())
                    };
                    let background = futures_util::future::join(consumer, worker);
                    match select(Box::pin(observe), Box::pin(background)).await {
                        Either::Left((result, _)) => result,
                        Either::Right(_) => panic!("refresh worker unexpectedly stopped"),
                    }
                };
                match select(
                    Box::pin(scenario),
                    Box::pin(glib::timeout_future(Duration::from_secs(5))),
                )
                .await
                {
                    Either::Left((result, _)) => result,
                    Either::Right(_) => {
                        Err("coalesced refresh stalled during a signal burst".into())
                    }
                }
            })
        })?
    }

    #[test]
    fn added_burst_does_not_block_refresh_or_lose_the_follow_up() -> TestResult<()> {
        run_burst_test(WifiSignal::Added)
    }

    #[test]
    fn removed_burst_does_not_block_refresh_or_lose_the_follow_up() -> TestResult<()> {
        run_burst_test(WifiSignal::Removed)
    }

    #[test]
    fn state_burst_does_not_block_refresh_or_lose_the_follow_up() -> TestResult<()> {
        run_burst_test(WifiSignal::StateChanged)
    }

    #[test]
    fn pending_device_update_is_not_downgraded_by_ap_updates() {
        let queue = RefreshQueue::default();
        queue.request(RefreshKind::Networks);
        queue.request(RefreshKind::Device);
        queue.request(RefreshKind::Networks);
        assert_eq!(queue.pending.take(), Some(RefreshKind::Device));
    }
}
