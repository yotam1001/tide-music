//! Fixed native audio processing and bounded output metadata. No executable input.
use serde_json::Value;
use tide_core::AudioDevice;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioOptions {
    pub normalize: bool,
    pub device: String,
    /// Index into [`EQUALIZER_PRESETS`]; 0 is off.
    pub equalizer: u8,
}
impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            normalize: false,
            device: "auto".into(),
            equalizer: 0,
        }
    }
}

/// Fixed equalizer presets as (name, lavfi chain). Boosting presets lower the
/// level first so the raised bands don't clip.
pub const EQUALIZER_PRESETS: [(&str, &str); 6] = [
    ("Off", ""),
    (
        "Bass boost",
        "volume=-5dB,equalizer=f=60:t=o:w=1.4:g=6,equalizer=f=150:t=o:w=1.4:g=3",
    ),
    (
        "Bass reducer",
        "equalizer=f=60:t=o:w=1.4:g=-6,equalizer=f=150:t=o:w=1.4:g=-3",
    ),
    (
        "Treble boost",
        "volume=-4dB,equalizer=f=6000:t=o:w=1.4:g=4,equalizer=f=12000:t=o:w=1.4:g=5",
    ),
    (
        "Vocal",
        "volume=-3dB,equalizer=f=250:t=o:w=1.2:g=-2,equalizer=f=2500:t=o:w=1.2:g=4",
    ),
    (
        "Loudness",
        "volume=-5dB,equalizer=f=60:t=o:w=1.4:g=5,equalizer=f=10000:t=o:w=1.4:g=4",
    ),
];
impl AudioOptions {
    pub fn valid(&self) -> bool {
        usize::from(self.equalizer) < EQUALIZER_PRESETS.len()
            && (self.device == "auto"
                || (self.device.starts_with("wasapi/")
                    && self.device.len() > 7
                    && self.device.len() <= 512
                    && !self.device.chars().any(char::is_control)))
    }
    /// Whether a change from `old` needs the audio filter chain replaced.
    pub(crate) fn filters_differ(&self, old: &Self) -> bool {
        self.normalize != old.normalize || self.equalizer != old.equalizer
    }
    pub(crate) fn filter(&self) -> String {
        let mut filters = Vec::new();
        if let Some((_, chain)) = EQUALIZER_PRESETS
            .get(usize::from(self.equalizer))
            .filter(|(_, chain)| !chain.is_empty())
        {
            filters.push(format!("@tide-equalizer:lavfi=[{chain}]"));
        }
        if self.normalize {
            filters.push("@tide-normalize:lavfi=[dynaudnorm=f=150:g=5:p=0.9:m=4]".into());
        }
        filters.join(",")
    }
}
pub(crate) fn devices(value: &Value) -> Vec<AudioDevice> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?;
            if name == "auto"
                || !(AudioOptions {
                    device: name.into(),
                    ..Default::default()
                })
                .valid()
            {
                return None;
            }
            let description = entry.get("description")?.as_str()?;
            if description.is_empty()
                || description.len() > 512
                || description.chars().any(char::is_control)
            {
                return None;
            }
            Some(AudioDevice {
                name: name.into(),
                description: description.into(),
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bounded_device_metadata_rejects_other_backends_and_control_characters() {
        let value = json!([{"name":"wasapi/123", "description":"Headphones"},
            {"name":"auto","description":"Default"}, {"name":"https://host","description":"bad"},
            {"name":"wasapi/abc\n", "description":"bad"}]);
        assert_eq!(
            devices(&value),
            vec![AudioDevice {
                name: "wasapi/123".into(),
                description: "Headphones".into()
            }]
        );
        assert!(devices(&json!([{"name":"wasapi/1","description":"x".repeat(513)}])).is_empty());
        assert!(AudioOptions::default().valid());
        assert!(AudioOptions::default().filter().is_empty());
        assert!(AudioOptions {
            normalize: true,
            ..Default::default()
        }
        .filter()
        .contains("dynaudnorm"));
    }
    #[test]
    fn equalizer_presets_chain_before_normalization_and_are_bounded() {
        let both = AudioOptions {
            normalize: true,
            equalizer: 1,
            ..Default::default()
        };
        let filter = both.filter();
        assert!(filter.starts_with("@tide-equalizer:lavfi=[volume=-5dB,equalizer="));
        assert!(filter.ends_with("dynaudnorm=f=150:g=5:p=0.9:m=4]"));
        assert!(both.filters_differ(&AudioOptions {
            normalize: true,
            ..Default::default()
        }));
        for index in 1..EQUALIZER_PRESETS.len() as u8 {
            let options = AudioOptions {
                equalizer: index,
                ..Default::default()
            };
            assert!(options.valid());
            assert!(options.filter().starts_with("@tide-equalizer:lavfi=["));
            assert!(!options.filter().contains("dynaudnorm"));
        }
        assert!(!AudioOptions {
            equalizer: EQUALIZER_PRESETS.len() as u8,
            ..Default::default()
        }
        .valid());
    }
}
