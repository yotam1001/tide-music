//! Fixed native audio processing and bounded output metadata. No executable input.
use serde_json::Value;
use tide_core::AudioDevice;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioOptions {
    pub normalize: bool,
    pub device: String,
}
impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            normalize: false,
            device: "auto".into(),
        }
    }
}
impl AudioOptions {
    pub fn valid(&self) -> bool {
        self.device == "auto"
            || (self.device.starts_with("wasapi/")
                && self.device.len() > 7
                && self.device.len() <= 512
                && !self.device.chars().any(char::is_control))
    }
    pub(crate) fn filter(&self) -> &'static str {
        if self.normalize {
            "@tide-normalize:lavfi=[dynaudnorm=f=150:g=5:p=0.9:m=4]"
        } else {
            ""
        }
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
                    normalize: false,
                    device: name.into(),
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
            device: "auto".into()
        }
        .filter()
        .contains("dynaudnorm"));
    }
}
