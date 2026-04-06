use std::path::{Path, PathBuf};
use std::time::Duration;

use edgecrab_types::Platform;
use tokio::process::Command;
use tracing::warn;

#[derive(Debug, Clone)]
pub(crate) struct PreparedVoiceAttachment {
    pub path: PathBuf,
    pub file_name: String,
    pub mime: &'static str,
    pub is_native_voice_note: bool,
    pub cleanup_after_send: bool,
    pub duration_secs: Option<f32>,
}

impl PreparedVoiceAttachment {
    pub async fn cleanup(&self) {
        if self.cleanup_after_send {
            let _ = tokio::fs::remove_file(&self.path).await;
        }
    }
}

fn voice_note_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("ogg" | "opus")
    )
}

fn audio_mime_for_extension(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("ogg" | "opus") => "audio/ogg",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        _ => "application/octet-stream",
    }
}

fn prefers_native_voice_note(platform: Platform) -> bool {
    matches!(platform, Platform::Telegram | Platform::Discord)
}

fn resolve_voice_attachment_path(
    original: &Path,
    platform: Platform,
    converted_path: anyhow::Result<Option<PathBuf>>,
) -> (PathBuf, bool, bool) {
    let prefers_native = prefers_native_voice_note(platform);
    if voice_note_extension(original) && prefers_native {
        return (original.to_path_buf(), true, false);
    }
    if !prefers_native {
        return (original.to_path_buf(), false, false);
    }

    match converted_path {
        Ok(Some(converted)) => (converted, true, true),
        Ok(None) | Err(_) => (original.to_path_buf(), false, false),
    }
}

async fn build_prepared_voice_attachment(
    prepared_path: PathBuf,
    native_voice_note: bool,
    cleanup_after_send: bool,
) -> anyhow::Result<PreparedVoiceAttachment> {
    let file_name = prepared_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(if native_voice_note {
            "voice-message.ogg"
        } else {
            "audio"
        })
        .to_string();
    let size_bytes = tokio::fs::metadata(&prepared_path).await?.len();
    let mime = if native_voice_note {
        "audio/ogg"
    } else {
        audio_mime_for_extension(&prepared_path)
    };

    Ok(PreparedVoiceAttachment {
        duration_secs: Some(estimate_audio_duration_secs(&prepared_path, size_bytes)),
        path: prepared_path,
        file_name,
        mime,
        is_native_voice_note: native_voice_note,
        cleanup_after_send,
    })
}

fn ffmpeg_binary() -> Option<String> {
    for candidate in [
        "ffmpeg",
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
    ] {
        if let Ok(path) = which::which(candidate) {
            return Some(path.to_string_lossy().into_owned());
        }
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

async fn convert_to_ogg_opus(input_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let Some(ffmpeg) = ffmpeg_binary() else {
        return Ok(None);
    };

    let output_path = std::env::temp_dir().join(format!(
        "edgecrab_voice_note_{}.ogg",
        uuid::Uuid::new_v4().simple()
    ));
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new(ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-i",
                &input_path.to_string_lossy(),
                "-vn",
                "-acodec",
                "libopus",
                "-b:a",
                "64k",
                &output_path.to_string_lossy(),
            ])
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ffmpeg voice-note conversion timed out after 30s"))?
    .map_err(|e| anyhow::anyhow!("failed to run ffmpeg for voice-note conversion: {e}"))?;

    if !output.status.success() || !output_path.is_file() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg voice-note conversion failed: {stderr}");
    }

    Ok(Some(output_path))
}

fn estimate_audio_duration_secs(path: &Path, size_bytes: u64) -> f32 {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("ogg" | "opus") => (size_bytes as f32 / 8_000.0).max(1.0),
        Some("mp3" | "m4a" | "aac") => (size_bytes as f32 / 16_000.0).max(1.0),
        Some("wav") => (size_bytes as f32 / 32_000.0).max(1.0),
        _ => 5.0,
    }
}

pub(crate) async fn prepare_voice_attachment(
    path: &str,
    platform: Platform,
) -> anyhow::Result<PreparedVoiceAttachment> {
    let original = PathBuf::from(path);
    if !original.is_file() {
        anyhow::bail!("voice attachment path does not exist: {path}");
    }

    let conversion = if prefers_native_voice_note(platform) && !voice_note_extension(&original) {
        convert_to_ogg_opus(&original).await
    } else {
        Ok(None)
    };
    if let Err(error) = conversion.as_ref() {
        warn!(
            platform = ?platform,
            path = %original.display(),
            error = %error,
            "voice-note conversion failed; falling back to the original audio attachment"
        );
    }

    let (prepared_path, native_voice_note, cleanup_after_send) =
        resolve_voice_attachment_path(&original, platform, conversion);
    build_prepared_voice_attachment(prepared_path, native_voice_note, cleanup_after_send).await
}

pub(crate) fn voice_delivery_doctor(platform: Platform) -> String {
    let ffmpeg = if ffmpeg_binary().is_some() {
        "available"
    } else {
        "missing"
    };
    match platform {
        Platform::Telegram => format!(
            "Voice delivery doctor\nPlatform: Telegram\nNative voice bubble: supported\nffmpeg: {ffmpeg}\nIf ffmpeg is available, EdgeCrab converts TTS audio to OGG/Opus so replies render as Telegram voice notes."
        ),
        Platform::Discord => format!(
            "Voice delivery doctor\nPlatform: Discord\nNative voice attachment: supported\nLive Discord voice channels: not implemented in EdgeCrab yet\nffmpeg: {ffmpeg}\nIf ffmpeg is available, EdgeCrab converts TTS audio to OGG/Opus and tries Discord's native voice-message API before falling back to a regular file upload."
        ),
        other => format!(
            "Voice delivery doctor\nPlatform: {other}\nNative voice-note optimization: not implemented for this platform\nffmpeg: {ffmpeg}\nEdgeCrab will still send audio replies when the adapter supports audio attachments."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_note_extension_detects_ogg_and_opus() {
        assert!(voice_note_extension(Path::new("/tmp/reply.ogg")));
        assert!(voice_note_extension(Path::new("/tmp/reply.opus")));
        assert!(!voice_note_extension(Path::new("/tmp/reply.mp3")));
    }

    #[test]
    fn audio_mime_matches_common_audio_types() {
        assert_eq!(
            audio_mime_for_extension(Path::new("voice.ogg")),
            "audio/ogg"
        );
        assert_eq!(
            audio_mime_for_extension(Path::new("voice.mp3")),
            "audio/mpeg"
        );
        assert_eq!(
            audio_mime_for_extension(Path::new("voice.wav")),
            "audio/wav"
        );
    }

    #[test]
    fn doctor_mentions_discord_voice_channel_gap_honestly() {
        let report = voice_delivery_doctor(Platform::Discord);
        assert!(report.contains("not implemented"));
        assert!(report.contains("Discord"));
    }

    #[test]
    fn native_voice_preference_is_limited_to_supported_platforms() {
        assert!(prefers_native_voice_note(Platform::Telegram));
        assert!(prefers_native_voice_note(Platform::Discord));
        assert!(!prefers_native_voice_note(Platform::Slack));
    }

    #[test]
    fn failed_conversion_falls_back_to_original_audio() {
        let (path, native, cleanup) = resolve_voice_attachment_path(
            Path::new("/tmp/reply.mp3"),
            Platform::Telegram,
            Err(anyhow::anyhow!("boom")),
        );
        assert_eq!(path, PathBuf::from("/tmp/reply.mp3"));
        assert!(!native);
        assert!(!cleanup);
    }

    #[test]
    fn successful_conversion_uses_native_voice_note() {
        let (path, native, cleanup) = resolve_voice_attachment_path(
            Path::new("/tmp/reply.mp3"),
            Platform::Discord,
            Ok(Some(PathBuf::from("/tmp/reply.ogg"))),
        );
        assert_eq!(path, PathBuf::from("/tmp/reply.ogg"));
        assert!(native);
        assert!(cleanup);
    }
}
