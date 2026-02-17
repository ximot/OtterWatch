// Re-export generated Protocol Buffer types
// Types will be used by MQTT client (mqtt_client.rs)
#[allow(dead_code)]
#[allow(clippy::derive_partial_eq_without_eq)]
mod otterwatch_v1 {
    include!("otterwatch.v1.rs");
}

#[allow(unused_imports)]
pub use otterwatch_v1::*;
