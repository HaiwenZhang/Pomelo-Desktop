//! Unit-aware canvas labels; source geometry remains in millimeters.
use pomelo_core::{
    i18n::{Message, MessageKey},
    interaction::ScaleBar,
    units::LengthUnit,
};

pub(super) fn scale_message(scale: ScaleBar, unit: LengthUnit) -> Message {
    let distance = unit.from_millimeters(scale.millimeters);
    // Keep fractional ticks visible; extreme scales use compact numeric notation.
    let value = if (1e-6..1e6).contains(&distance) {
        format!("{distance:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    } else {
        format!("{distance:.0e}")
    };
    Message::new(match unit {
        LengthUnit::Millimeters => MessageKey::ScaleDistance,
        LengthUnit::Mils => MessageKey::ScaleDistanceMils,
    })
    .arg("distance", value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::{i18n::Locale, interaction::Camera};

    #[test]
    fn fractional_scale_labels_match_projected_geometry_in_both_units() {
        for exponent in -3..=7 {
            let camera = Camera {
                pixels_per_mm: 3.7 * 10_f64.powi(exponent),
                ..Camera::default()
            };
            for unit in [LengthUnit::Millimeters, LengthUnit::Mils] {
                let scale = camera.scale_bar_in_unit(100.0, unit).unwrap();
                let message = scale_message(scale, unit);
                let shown: f64 = message.args["distance"].to_string().parse().unwrap();
                let projected = unit.to_millimeters(shown) * camera.pixels_per_mm;
                assert!(
                    shown > 0.0 && (projected - scale.pixels).abs() < 1e-8,
                    "shown={shown}, projected={projected}, scale={scale:?}"
                );
                for locale in Locale::ALL {
                    assert!(message.render(locale).is_ok());
                }
            }
        }
    }
}
