use std::collections::HashMap;
use zbus::zvariant::Value;

/// Build a NM connection settings dict for connecting to a WPA-PSK secured network.
pub fn build_wpa_psk_settings<'a>(ssid: &str, password: &'a str) -> HashMap<String, HashMap<String, Value<'a>>> {
    let mut settings: HashMap<String, HashMap<String, Value>> = HashMap::new();

    // connection section
    let mut connection = HashMap::new();
    connection.insert("type".to_string(), Value::from("802-11-wireless"));
    settings.insert("connection".to_string(), connection);

    // 802-11-wireless section
    let mut wireless = HashMap::new();
    wireless.insert("ssid".to_string(), Value::from(ssid.as_bytes().to_vec()));
    settings.insert("802-11-wireless".to_string(), wireless);

    // 802-11-wireless-security section
    let mut security = HashMap::new();
    security.insert("key-mgmt".to_string(), Value::from("wpa-psk"));
    security.insert("psk".to_string(), Value::from(password));
    settings.insert("802-11-wireless-security".to_string(), security);

    settings
}

/// Build a NM connection settings dict for connecting to a SAE (WPA3) network.
pub fn build_wpa3_settings<'a>(ssid: &str, password: &'a str) -> HashMap<String, HashMap<String, Value<'a>>> {
    let mut settings: HashMap<String, HashMap<String, Value>> = HashMap::new();

    let mut connection = HashMap::new();
    connection.insert("type".to_string(), Value::from("802-11-wireless"));
    settings.insert("connection".to_string(), connection);

    let mut wireless = HashMap::new();
    wireless.insert("ssid".to_string(), Value::from(ssid.as_bytes().to_vec()));
    settings.insert("802-11-wireless".to_string(), wireless);

    let mut security = HashMap::new();
    security.insert("key-mgmt".to_string(), Value::from("sae"));
    security.insert("psk".to_string(), Value::from(password));
    settings.insert("802-11-wireless-security".to_string(), security);

    settings
}

/// Build an empty settings dict (for open networks — NM fills in the rest).
pub fn build_open_settings() -> HashMap<String, HashMap<String, Value<'static>>> {
    HashMap::new()
}
