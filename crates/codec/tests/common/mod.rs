//! Helpers shared by the JSON integration tests.

use tidepool_codec::json::Value;

/// Equality that also tells `0.0` from `-0.0`, which `==` treats as equal.
pub(crate) fn identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) if !a.is_integer() && !b.is_integer() => {
            a.as_f64().to_bits() == b.as_f64().to_bits()
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| identical(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((ka, va), (kb, vb))| ka == kb && identical(va, vb))
        }
        _ => a == b,
    }
}
