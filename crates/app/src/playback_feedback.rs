//! UI copy comes from structured engine state, never from raw helper output.
use tide_core::PlaybackSnapshot;

pub(crate) fn hint(p: &PlaybackSnapshot) -> String {
    if p.error.is_some() {
        return if p.output_unavailable {
            "Audio output unavailable"
        } else {
            "Playback stopped"
        }
        .into();
    }
    if p.loading {
        return if p.retry_attempt > 0 {
            format!("Retrying playback ({}/2)…", p.retry_attempt.min(2))
        } else {
            "Loading audio…".into()
        };
    }
    if p.buffering && !p.paused {
        return "Buffering…".into();
    }
    if p.paused && p.duration > 0.0 && !p.ended {
        return "Paused".into();
    }
    String::new()
}

pub(crate) fn quality(p: &PlaybackSnapshot) -> String {
    let mut parts = Vec::new();
    if let Some(codec) = &p.audio_codec {
        parts.push(match codec.as_str() {
            "opus" => "Opus".into(),
            "aac" => "AAC".into(),
            "flac" => "FLAC".into(),
            "mp3" => "MP3".into(),
            _ => codec.clone(),
        });
    }
    if let Some(bits) = p.audio_bitrate_bps {
        // mpv reports a measured packet bitrate; VBR values are approximate.
        parts.push(if bits >= 1000 {
            format!("~{} kbps", bits.saturating_add(500) / 1000)
        } else {
            format!("~{bits} bps")
        });
    }
    if let Some(rate) = p.audio_sample_rate {
        parts.push(format!(
            "{} kHz",
            if rate % 1000 == 0 {
                (rate / 1000).to_string()
            } else {
                format!("{:.1}", rate as f64 / 1000.0)
            }
        ));
    }
    parts.join(" · ")
}

pub(crate) fn failure(p: &PlaybackSnapshot) -> (&'static str, &'static str) {
    if p.output_unavailable {
        (
            "Audio output unavailable. Choose Settings → Audio, then retry.",
            "Retry audio",
        )
    } else {
        (
            "This song could not play after two retries. Retry the song or choose another track.",
            "Retry song",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn states_distinguish_loading_buffering_pause_retries_and_output_failure() {
        let mut p = PlaybackSnapshot::default();
        assert!(hint(&p).is_empty());
        assert!(quality(&p).is_empty());
        p.loading = true;
        assert_eq!(hint(&p), "Loading audio…");
        p.retry_attempt = 1;
        assert_eq!(hint(&p), "Retrying playback (1/2)…");
        p.loading = false;
        p.buffering = true;
        p.paused = false;
        assert_eq!(hint(&p), "Buffering…");
        p.paused = true;
        p.duration = 60.0;
        assert_eq!(hint(&p), "Paused");
        p.output_unavailable = true;
        p.error = Some("private source must never be displayed".into());
        assert_eq!(hint(&p), "Audio output unavailable");
        assert_eq!(failure(&p).1, "Retry audio");
        assert!(!failure(&p).0.contains("private"));
        p.output_unavailable = false;
        assert_eq!(failure(&p).1, "Retry song");
        p.error = None;
        p.ended = true;
        assert!(hint(&p).is_empty());
    }
    #[test]
    fn quality_is_observed_data_without_a_premium_or_fixed_quality_claim() {
        let p = PlaybackSnapshot {
            audio_codec: Some("opus".into()),
            audio_bitrate_bps: Some(159_500),
            audio_sample_rate: Some(48_000),
            ..Default::default()
        };
        assert_eq!(quality(&p), "Opus · ~160 kbps · 48 kHz");
        assert!(!quality(&p).contains("Premium"));
    }
}
