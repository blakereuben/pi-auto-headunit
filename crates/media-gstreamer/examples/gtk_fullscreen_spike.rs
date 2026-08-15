//! On-device GTK4/GStreamer rendering spike.
//!
//! Answers `ARCHITECTURE.md` §4's explicit gate: "GTK/GStreamer
//! integration, render-buffer sharing, and 720p/1080p latency must pass an
//! on-device architecture spike before this choice is locked in." Nothing
//! in this repo has linked against GTK before this. This is a disposable
//! spike, not a permanent CLI command, not live-phone integration, and not
//! the real `ui-model`/`ui-gtk` crates `ARCHITECTURE.md` describes — see
//! the plan history for the full scope decision. The already-proven direct
//! `waylandsink` path in `apps/aa-headunit-diagnostics/src/
//! auth_discovery_probe.rs` (real video confirmed on the head unit's own
//! display) is completely untouched; this uses a separate `RenderSink`
//! variant (`RenderSink::Gtk4Paintable`) added purely for this example.
//!
//! Opens a full-screen GTK4 window (no VNC involved — this runs directly
//! against the Pi's own Wayland session) and renders a self-generated,
//! moving synthetic H.264 clip through `gtk4paintablesink`, bridged into a
//! `gtk::Picture` via that element's `paintable` `GObject` property
//! (`VideoRenderPipeline::gtk4_paintable_property`). Frames are pushed
//! from a background thread, real-time-paced (~30fps), so an operator can
//! visually judge smoothness over the whole run; `appsrc` buffer pushes
//! are thread-safe, so no GTK/GDK object is ever touched off the main
//! thread. The run ends on its own once playback finishes — no operator
//! Ctrl-C required.
//!
//! A first real-hardware run rendered the moving pattern correctly but
//! showed black stretches before and after it, both traced to this
//! example's own structure rather than GTK4/GStreamer rendering itself:
//! (1) the synthetic clip used to be encoded to memory in full *before*
//! any frame was pushed, so nothing appeared on screen until the whole
//! encode finished; fixed by pulling one encoded access unit and pushing
//! it into the render pipeline immediately, in the same loop
//! (`stream_synthetic_h264_frames`). (2) the render pipeline used to be
//! dropped (tearing down the display) as soon as the push loop finished,
//! independent of a fixed-duration window-close timer racing separately —
//! fixed by closing the window from an `AtomicBool` flag set only once
//! playback genuinely finishes, so the pipeline teardown and the window
//! close happen together instead of leaving a black gap between them.
//!
//! Requires `gstreamer1.0-gtk4` (the plugin providing `gtk4paintablesink`)
//! and a reachable Wayland compositor (`WAYLAND_DISPLAY`). No `sudo`
//! needed — this touches only the Wayland session, not raw USB or
//! root-owned credentials, unlike the `usb`/`developer` diagnostics
//! commands. Run with `cargo run --example gtk_fullscreen_spike -p
//! media-gstreamer`.

#[cfg(target_os = "linux")]
fn main() {
    use gtk4::prelude::*;
    use gtk4::{Application, ApplicationWindow, Picture, glib};
    use media_gstreamer::{GstreamerBackend, RenderSink};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    const SPIKE_DURATION_SECONDS: u32 = 30;
    const FRAMES_PER_SECOND: u32 = 30;
    /// Pure safety net (this project's established "never hang"
    /// discipline) — the `AtomicBool` poll below closes the window right
    /// when playback actually finishes; this only fires if something
    /// upstream got stuck.
    const HANG_SAFETY_NET_SECONDS: u32 = 90;

    let application = Application::builder()
        .application_id("dev.pi-auto-headunit.gtk-fullscreen-spike")
        .build();

    application.connect_activate(|application| {
        let backend = GstreamerBackend::new().expect("gstreamer available on this host");
        let pipeline = backend
            .build_video_render_pipeline(&h264_capability(), RenderSink::Gtk4Paintable)
            .expect(
                "gtk4paintablesink pipeline builds (requires gstreamer1.0-gtk4 to be installed)",
            );
        let paintable = pipeline
            .gtk4_paintable_property()
            .expect("gtk4paintablesink element present in the pipeline")
            .get::<gtk4::gdk::Paintable>()
            .expect("\"paintable\" property is a GdkPaintable");

        let picture = Picture::new();
        picture.set_paintable(Some(&paintable));

        let window = ApplicationWindow::builder()
            .application(application)
            .title("pi-auto-headunit GTK4 rendering spike")
            .child(&picture)
            .build();
        window.fullscreen();
        window.present();

        pipeline
            .start()
            .expect("pipeline starts (needs a reachable Wayland compositor)");

        let playback_done = Arc::new(AtomicBool::new(false));
        let playback_done_writer = Arc::clone(&playback_done);
        thread::spawn(move || {
            stream_synthetic_h264_frames(
                SPIKE_DURATION_SECONDS * FRAMES_PER_SECOND,
                Duration::from_secs(1) / FRAMES_PER_SECOND,
                &pipeline,
            );
            drop(pipeline);
            playback_done_writer.store(true, Ordering::Release);
        });

        let application_for_poll = application.clone();
        let _quit_poll_id = glib::timeout_add_local(Duration::from_millis(100), move || {
            if playback_done.load(Ordering::Acquire) {
                application_for_poll.quit();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });

        let application_for_timeout = application.clone();
        let _timeout_id = glib::timeout_add_seconds_local(HANG_SAFETY_NET_SECONDS, move || {
            application_for_timeout.quit();
            glib::ControlFlow::Break
        });
    });

    let _exit_code = application.run();
}

/// `avdec_h264`/software decode, matching `crates/media-gstreamer/src/
/// render.rs`'s own test fixture capability (`h264_capability`) — kept as
/// a separate copy here since that one is `mod tests`-private, not public
/// API, and this is the only other caller.
#[cfg(target_os = "linux")]
fn h264_capability() -> media_api::DecoderCapability {
    media_api::DecoderCapability {
        id: "gstreamer:avdec_h264".into(),
        codec: media_api::VideoCodec::H264,
        kind: media_api::DecoderKind::Software,
        maximum_width: 1280,
        maximum_height: 720,
        maximum_frames_per_second: 30,
    }
}

/// Encodes a self-generated, moving Annex-B H.264 clip
/// (`videotestsrc pattern=ball ! openh264enc ! h264parse ! appsink`) —
/// `pattern=ball` (rather than the default static SMPTE bars) so an
/// operator can actually judge playback smoothness, not just color
/// correctness — and pushes each access unit into `render` as soon as
/// it's pulled, paced by `frame_interval`, rather than encoding the whole
/// clip to memory first (that batching caused a real black-screen delay
/// on the first real-hardware run — see this file's module doc comment).
/// Never derived from a real phone capture — see `CLAUDE.md`'s
/// user-content rule. The encode side mirrors `render.rs`'s own
/// `synthetic_h264_access_units` test helper; duplicated rather than
/// exported as public API for this one disposable caller. With
/// `config-interval=-1`, every access unit already carries in-band
/// parameter sets, so the first one pushed doubles as `CodecConfig`.
#[cfg(target_os = "linux")]
fn stream_synthetic_h264_frames(
    count: u32,
    frame_interval: std::time::Duration,
    render: &media_gstreamer::VideoRenderPipeline,
) {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;
    use std::thread;

    gst::init().expect("gstreamer available on this host");
    let description = format!(
        "videotestsrc num-buffers={count} pattern=ball \
         ! video/x-raw,width=1280,height=720,framerate=30/1 \
         ! openh264enc \
         ! h264parse config-interval=-1 \
         ! video/x-h264,stream-format=byte-stream,alignment=au \
         ! appsink name=sink emit-signals=false sync=false"
    );
    let encode_pipeline = gst::parse::launch(&description)
        .expect("fixture pipeline parses")
        .downcast::<gst::Pipeline>()
        .expect("fixture graph is a Pipeline");
    let appsink = encode_pipeline
        .by_name("sink")
        .expect("named appsink present")
        .downcast::<gst_app::AppSink>()
        .expect("sink is an AppSink");
    encode_pipeline
        .set_state(gst::State::Playing)
        .expect("fixture pipeline starts");

    let mut pushed_any = false;
    let mut index: u64 = 0;
    while let Ok(sample) = appsink.pull_sample() {
        let buffer = sample.buffer().expect("sample has a buffer");
        let map = buffer.map_readable().expect("buffer is readable");
        let access_unit = map.as_slice();
        let push_result = if index == 0 {
            render.push_codec_config(access_unit)
        } else {
            render.push_frame(access_unit, index * 33_333)
        };
        if let Err(error) = push_result {
            eprintln!("frame push failed: {error}");
            break;
        }
        if let Some(error) = render.poll_bus_error() {
            eprintln!("pipeline error: {error}");
            break;
        }
        pushed_any = true;
        index += 1;
        thread::sleep(frame_interval);
    }
    let _ = encode_pipeline.set_state(gst::State::Null);
    assert!(pushed_any, "fixture produced no access units");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("gtk_fullscreen_spike requires target_os = \"linux\"");
}
