//! Compact System detail page widgets.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

use crate::domain::system::{SystemSnapshot, format_duration, format_memory_usage};

#[derive(Clone)]
pub(crate) struct SystemWidgets {
    pub(crate) container: GtkBox,
    pub(crate) status: Label,
    pub(crate) cpu_value: Label,
    pub(crate) temperature_row: GtkBox,
    pub(crate) temperature_value: Label,
    pub(crate) memory_value: Label,
    pub(crate) swap_row: GtkBox,
    pub(crate) swap_value: Label,
    pub(crate) disk_value: Label,
    pub(crate) network_row: GtkBox,
    pub(crate) network_value: Label,
    pub(crate) uptime_value: Label,
    pub(crate) gpu_section: GtkBox,
    pub(crate) gpu_value: Label,
}

impl SystemWidgets {
    pub(crate) fn new() -> Self {
        let container = GtkBox::new(Orientation::Vertical, 8);
        container.add_css_class("cc-system-page");
        container.set_margin_start(16);
        container.set_margin_end(16);
        container.set_margin_top(8);
        container.set_margin_bottom(12);

        let status = Label::new(Some("Loading system metrics…"));
        status.add_css_class("cc-detail-status");
        status.set_halign(Align::Start);
        status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let overview = section("Overview");
        let (cpu_row, cpu_value) = metric_row("CPU usage");
        let (temperature_row, temperature_value) = metric_row("CPU / SoC temperature");
        let (memory_row, memory_value) = metric_row("Memory");
        let (swap_row, swap_value) = metric_row("Swap");
        let (disk_row, disk_value) = metric_row("Root filesystem");
        let (network_row, network_value) = metric_row("Network");
        let (uptime_row, uptime_value) = metric_row("Uptime");
        for row in [
            cpu_row,
            temperature_row.clone(),
            memory_row,
            swap_row.clone(),
            disk_row,
            network_row.clone(),
            uptime_row,
        ] {
            overview.append(&row);
        }
        container.append(&overview);

        let gpu_section = GtkBox::new(Orientation::Vertical, 4);
        gpu_section.add_css_class("cc-system-section");
        gpu_section.set_visible(false);
        let gpu_title = Label::new(Some("GPU"));
        gpu_title.add_css_class("cc-system-section-title");
        gpu_title.set_halign(Align::Start);
        let (gpu_row, gpu_value) = metric_row("Utilization / temperature");
        gpu_section.append(&gpu_title);
        gpu_section.append(&gpu_row);
        container.append(&gpu_section);

        Self {
            container,
            status,
            cpu_value,
            temperature_row,
            temperature_value,
            memory_value,
            swap_row,
            swap_value,
            disk_value,
            network_row,
            network_value,
            uptime_value,
            gpu_section,
            gpu_value,
        }
    }

    pub(crate) fn render(&self, snapshot: &SystemSnapshot) {
        self.cpu_value.set_text(&optional_percent(snapshot.cpu_usage_percent));
        self.temperature_row.set_visible(snapshot.cpu_temperature_c.is_some());
        self.temperature_value.set_text(
            &snapshot.cpu_temperature_c.map(|value| format!("{value:.1} °C")).unwrap_or_default(),
        );
        self.memory_value.set_text(&memory_text(
            snapshot.memory_used_bytes,
            snapshot.memory_total_bytes,
            snapshot.memory_used_percent,
        ));
        self.swap_row
            .set_visible(snapshot.swap_used_bytes.is_some() && snapshot.swap_total_bytes.is_some());
        self.swap_value.set_text(&memory_text(
            snapshot.swap_used_bytes,
            snapshot.swap_total_bytes,
            snapshot.swap_used_percent,
        ));
        self.disk_value.set_text(&optional_percent(snapshot.root_used_percent));
        let network_visible =
            snapshot.rx_bytes_per_second.is_some() || snapshot.tx_bytes_per_second.is_some();
        self.network_row.set_visible(network_visible);
        self.network_value.set_text(&format!(
            "↓ {} · ↑ {}",
            format_rate(snapshot.rx_bytes_per_second),
            format_rate(snapshot.tx_bytes_per_second)
        ));
        self.uptime_value.set_text(
            &snapshot
                .uptime_seconds
                .map(format_duration)
                .unwrap_or_else(|| "Unavailable".to_string()),
        );

        if let Some(gpu) = snapshot.gpu.filter(|gpu| gpu.usable()) {
            self.gpu_section.set_visible(true);
            let utilization = gpu
                .utilization_percent
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| "Unavailable".to_string());
            let temperature = gpu
                .temperature_c
                .map(|value| format!("{value:.1} °C"))
                .unwrap_or_else(|| "Unavailable".to_string());
            self.gpu_value.set_text(&format!("{utilization} · {temperature}"));
        } else {
            self.gpu_section.set_visible(false);
        }
        self.status.set_text("System metrics updated");
    }
}

fn section(title: &str) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 4);
    section.add_css_class("cc-system-section");
    let title = Label::new(Some(title));
    title.add_css_class("cc-system-section-title");
    title.set_halign(Align::Start);
    section.append(&title);
    section
}

fn metric_row(title: &str) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("cc-metric-row");
    let title_label = Label::new(Some(title));
    title_label.set_halign(Align::Start);
    title_label.set_hexpand(true);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_label.add_css_class("cc-metric-title");
    let value = Label::new(Some("Unavailable"));
    value.set_halign(Align::End);
    value.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    value.add_css_class("cc-metric-value");
    row.append(&title_label);
    row.append(&value);
    (row, value)
}

fn optional_percent(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "Unavailable".to_string())
}

fn memory_text(used: Option<u64>, total: Option<u64>, percent: Option<f64>) -> String {
    let Some(text) = used.and_then(|used| total.and_then(|total| format_memory_usage(used, total)))
    else {
        return "Unavailable".to_string();
    };
    match percent {
        Some(percent) if percent.is_finite() => format!("{text} ({percent:.1}%)"),
        _ => text,
    }
}

fn format_rate(value: Option<f64>) -> String {
    let Some(value) = value else { return "Unavailable".to_string() };
    if value >= 1_000_000_000.0 {
        format!("{:.1} GB/s", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.1} MB/s", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1} kB/s", value / 1_000.0)
    } else {
        format!("{value:.0} B/s")
    }
}
