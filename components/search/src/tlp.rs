use crate::model::Tlp;

#[derive(Debug, Clone, Copy)]
pub struct TlpThresholds {
    pub red: f32,
    pub yellow: f32,
}

impl Default for TlpThresholds {
    fn default() -> Self {
        Self {
            red: 0.95,
            yellow: 0.75,
        }
    }
}

/// TLP banding for the result-set as a whole, given the top hit's blended
/// score and an `exact_alias_match` flag (set by the engine when the query
/// matches a name or alias exactly after normalization).
///
/// - RED: exact name/alias match, OR top score ≥ red threshold.
/// - YELLOW: top score in [yellow, red).
/// - GREEN: everything else, including empty result sets.
pub fn band(top_score: Option<f32>, exact_alias_match: bool, t: &TlpThresholds) -> Tlp {
    if exact_alias_match {
        return Tlp::Red;
    }
    match top_score {
        Some(s) if s >= t.red => Tlp::Red,
        Some(s) if s >= t.yellow => Tlp::Yellow,
        _ => Tlp::Green,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_on_empty() {
        assert_eq!(band(None, false, &TlpThresholds::default()), Tlp::Green);
    }

    #[test]
    fn green_on_low_score() {
        assert_eq!(band(Some(0.5), false, &TlpThresholds::default()), Tlp::Green);
    }

    #[test]
    fn yellow_band() {
        let t = TlpThresholds::default();
        assert_eq!(band(Some(0.75), false, &t), Tlp::Yellow);
        assert_eq!(band(Some(0.94), false, &t), Tlp::Yellow);
    }

    #[test]
    fn red_band() {
        let t = TlpThresholds::default();
        assert_eq!(band(Some(0.95), false, &t), Tlp::Red);
        assert_eq!(band(Some(1.0), false, &t), Tlp::Red);
    }

    #[test]
    fn exact_alias_forces_red() {
        let t = TlpThresholds::default();
        assert_eq!(band(Some(0.1), true, &t), Tlp::Red);
        assert_eq!(band(None, true, &t), Tlp::Red);
    }

    #[test]
    fn custom_thresholds() {
        let t = TlpThresholds {
            red: 0.99,
            yellow: 0.5,
        };
        assert_eq!(band(Some(0.6), false, &t), Tlp::Yellow);
        assert_eq!(band(Some(0.98), false, &t), Tlp::Yellow);
        assert_eq!(band(Some(0.99), false, &t), Tlp::Red);
    }
}
