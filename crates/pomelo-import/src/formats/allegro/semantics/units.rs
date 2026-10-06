//! Source coordinate conversion; millimetres are the shared board-space unit.

use crate::ImportError;

pub fn millimetres_per_unit(units: u16, divisor: u32) -> Result<f64, ImportError> {
    if divisor == 0 {
        return Err(ImportError::InvalidDivisor);
    }
    let base = match units {
        1 => 0.0254,
        2 => 25.4,
        3 => 1.0,
        4 => 10.0,
        5 => 0.001,
        _ => return Err(ImportError::UnsupportedUnits { units, divisor }),
    };
    Ok(base / f64::from(divisor))
}
