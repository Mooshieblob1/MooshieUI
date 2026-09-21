//! Minimal host state for testing the production prerequisite installer unchanged.
//! This validates the toolchain, not Tauri/WebView, Gatekeeper, or music generation.
#![allow(dead_code)]

#[path = "../../../src-tauri/src/media_tools.rs"]
mod media_tools;

mod config {
    pub fn app_data_dir() -> Option<std::path::PathBuf> {
        let base = std::path::PathBuf::from(std::env::var_os("MOOSHIE_MEDIA_TOOLS_SMOKE_ROOT")?);
        base.is_absolute().then_some(base)
    }
}

mod state {
    #[derive(Default)]
    pub struct Config {
        pub network_proxy: Option<String>,
    }

    pub struct AppState {
        pub media_tools: crate::media_tools::MediaTools,
        pub config: tokio::sync::RwLock<Config>,
        pub http_client: reqwest::Client,
    }
}

#[cfg(test)]
mod native {
    use super::*;
    use std::{path::Path, sync::Arc, time::Duration};

    async fn run(path: &Path, args: &[&str]) -> String {
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(path)
                .args(args)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("Tool timed out")
        .expect("Tool did not start");
        assert!(
            result.status.success(),
            "{}: {}",
            path.display(),
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8_lossy(&result.stdout).into_owned()
    }

    #[tokio::test]
    #[ignore = "Run after the production live_install_and_offline_reuse test with an isolated smoke root"]
    async fn startup_reuses_tools_offline_and_runs_media_pipeline() {
        let root = config::app_data_dir().expect("Set MOOSHIE_MEDIA_TOOLS_SMOKE_ROOT");
        let installed = root.join("bin/media");
        assert!(installed.is_dir(), "Run the cold installation test first");
        let state = Arc::new(state::AppState {
            media_tools: media_tools::MediaTools::default(),
            config: tokio::sync::RwLock::new(state::Config {
                network_proxy: Some("http://127.0.0.1:1".into()),
            }),
            http_client: reqwest::Client::builder()
                .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
                .build()
                .unwrap(),
        });
        media_tools::start(state.clone());
        media_tools::start(state.clone()); // Must not run overlapping installation jobs.
        tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                let status = state.media_tools.status();
                assert_ne!(status["status"], "error", "{status}");
                assert_ne!(
                    status["status"], "retrying",
                    "Cached startup attempted a download: {status}"
                );
                if status["available"] == true {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("Cached startup timed out");
        for tool in ["yt-dlp", "ffmpeg", "deno", "node"] {
            let path = state.media_tools.path(tool).unwrap();
            assert!(
                path.starts_with(&installed),
                "Used a system {tool} instead of its managed installation"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o755
                );
            }
        }
        let node = state.media_tools.path("node").unwrap();
        let deno = state.media_tools.path("deno").unwrap();
        assert_eq!(run(&node, &["-e", "console.log(6 * 7)"]).await.trim(), "42");
        assert_eq!(
            run(&deno, &["eval", "console.log(6 * 7)"]).await.trim(),
            "42"
        );

        let ffmpeg = state.media_tools.path("ffmpeg").unwrap();
        let audio = root.join("synthetic.mp3");
        run(
            &ffmpeg,
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.25",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "192k",
                "-y",
                audio.to_str().unwrap(),
            ],
        )
        .await;
        assert!(std::fs::metadata(&audio).unwrap().len() > 1000);
        run(
            &ffmpeg,
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                audio.to_str().unwrap(),
                "-f",
                "null",
                "-",
            ],
        )
        .await;
        std::fs::remove_file(audio).unwrap();
        // The external site is deliberately excluded: offline cached startup must work.
        let downloader = state.media_tools.path("yt-dlp").unwrap();
        run(
            &downloader,
            &[
                "--ignore-config",
                "--no-cache-dir",
                "--no-js-runtimes",
                "--js-runtimes",
                &format!("deno:{}", deno.display()),
                "--js-runtimes",
                &format!("node:{}", node.display()),
                "--ffmpeg-location",
                ffmpeg.to_str().unwrap(),
                "--list-extractors",
            ],
        )
        .await;
        media_tools::shutdown(&state).await;
        assert!(std::fs::read_dir(installed).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".install-")));
        println!("PASS: cached offline startup, explicit managed paths, Unix permissions, Node/Deno execution, MP3 encode/decode, and shutdown");
    }
}
