//! Display units; source geometry always remains in millimeters.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Millimeters,
    Mils,
}

impl LengthUnit {
    pub fn millimeters_per_unit(self) -> f64 {
        match self {
            Self::Millimeters => 1.0,
            Self::Mils => 0.0254,
        }
    }

    pub fn from_millimeters(self, length: f64) -> f64 {
        length / self.millimeters_per_unit()
    }

    pub fn to_millimeters(self, length: f64) -> f64 {
        length * self.millimeters_per_unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inch_is_one_thousand_mils() {
        assert!((LengthUnit::Mils.from_millimeters(25.4) - 1000.0).abs() < 1e-10);
        assert!((LengthUnit::Mils.to_millimeters(1000.0) - 25.4).abs() < 1e-10);
    }

    #[test]
    fn signed_coordinates_round_trip_in_both_units() {
        for unit in [LengthUnit::Millimeters, LengthUnit::Mils] {
            for value in [-2000.0, -0.0254, 0.0, 0.001, 5000.0] {
                assert!((unit.to_millimeters(unit.from_millimeters(value)) - value).abs() < 1e-9);
            }
        }
    }
}
