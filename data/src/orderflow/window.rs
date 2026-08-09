use serde::{Deserialize, Serialize};

use super::SpeedConfig;

/// User-selectable rolling window used by Trade Speed and Book Speed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum SpeedWindow {
    Ms250,
    Ms500,
    #[default]
    S1,
    S2,
    S5,
    S10,
}

impl SpeedWindow {
    pub const ALL: [Self; 6] = [
        Self::Ms250,
        Self::Ms500,
        Self::S1,
        Self::S2,
        Self::S5,
        Self::S10,
    ];

    pub const fn millis(self) -> u64 {
        match self {
            Self::Ms250 => 250,
            Self::Ms500 => 500,
            Self::S1 => 1_000,
            Self::S2 => 2_000,
            Self::S5 => 5_000,
            Self::S10 => 10_000,
        }
    }
}

impl From<SpeedWindow> for SpeedConfig {
    fn from(value: SpeedWindow) -> Self {
        SpeedConfig::new(value.millis())
    }
}

impl std::fmt::Display for SpeedWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ms250 => write!(f, "250ms"),
            Self::Ms500 => write!(f, "500ms"),
            Self::S1 => write!(f, "1s"),
            Self::S2 => write!(f, "2s"),
            Self::S5 => write!(f, "5s"),
            Self::S10 => write!(f, "10s"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_windows_are_strictly_increasing() {
        let values: Vec<_> = SpeedWindow::ALL
            .into_iter()
            .map(SpeedWindow::millis)
            .collect();
        assert_eq!(values, vec![250, 500, 1_000, 2_000, 5_000, 10_000]);
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn config_conversion_preserves_window() {
        for window in SpeedWindow::ALL {
            let config: SpeedConfig = window.into();
            assert_eq!(config.window_ms, window.millis());
        }
    }
}
