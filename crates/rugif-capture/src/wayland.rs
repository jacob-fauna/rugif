use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use error_stack::{Result, ResultExt};
use rugif_core::{CaptureFrame, Region};

use crate::{CaptureError, CaptureReceiver, ScreenCapture};

/// Wayland screen capture backend using XDG Desktop Portal + PipeWire.
///
/// The tokio runtime and portal session are kept alive for the lifetime of this
/// struct — dropping them invalidates the PipeWire node.
pub struct WaylandCapture {
    stop_flag: Option<Arc<AtomicBool>>,
    capture_thread: Option<thread::JoinHandle<()>>,
    screen_width: u32,
    screen_height: u32,
    pw_node_id: u32,
    /// Kept alive so the portal session (and thus the PipeWire node) remains valid.
    _rt: tokio::runtime::Runtime,
}

impl WaylandCapture {
    pub fn new() -> Result<Self, CaptureError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .change_context(CaptureError::Connection)?;

        let (node_id, width, height) = rt
            .block_on(request_screencast())
            .change_context(CaptureError::Connection)?;

        tracing::info!("portal screencast: node_id={node_id}, {width}x{height}");

        Ok(Self {
            stop_flag: None,
            capture_thread: None,
            screen_width: width,
            screen_height: height,
            pw_node_id: node_id,
            _rt: rt,
        })
    }
}

/// Request a screencast session via XDG Desktop Portal.
/// The session is intentionally leaked (via `mem::forget`) so the PipeWire node
/// stays valid for the lifetime of the process.
async fn request_screencast() -> std::result::Result<(u32, u32, u32), CaptureError> {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
    use ashpd::desktop::PersistMode;

    let proxy = Screencast::new()
        .await
        .map_err(|e| CaptureError::Screenshot(format!("screencast proxy: {e}")))?;

    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| CaptureError::Screenshot(format!("create_session: {e}")))?;

    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(enumflags2::BitFlags::from(SourceType::Monitor))
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|e| CaptureError::Screenshot(format!("select_sources: {e}")))?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| CaptureError::Screenshot(format!("start: {e}")))?
        .response()
        .map_err(|e| CaptureError::Screenshot(format!("start response: {e}")))?;

    let streams = response.streams();
    let stream = streams
        .first()
        .ok_or(CaptureError::Screenshot("no streams returned".into()))?;

    let node_id = stream.pipe_wire_node_id();
    let (width, height) = stream.size().unwrap_or((1920, 1080));

    // Leak the session so the PipeWire node stays valid.
    // The session would otherwise be dropped here, causing the compositor
    // to tear down the screencast and invalidate the node.
    std::mem::forget(session);

    Ok((node_id, width as u32, height as u32))
}

/// Build the PipeWire stream format params for video capture.
fn build_video_params(width: u32, height: u32) -> Vec<u8> {
    use pipewire::spa;

    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGB
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle { width, height },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );

    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .expect("failed to serialize video params")
    .0
    .into_inner()
}

/// Connect a PipeWire stream to a node with proper format params.
fn connect_pw_stream(
    stream: &pipewire::stream::Stream,
    node_id: u32,
    screen_width: u32,
    screen_height: u32,
) {
    use pipewire as pw;
    use pipewire::stream::StreamFlags;

    let param_bytes = build_video_params(screen_width, screen_height);
    let mut params = [pw::spa::pod::Pod::from_bytes(&param_bytes).unwrap()];

    stream
        .connect(
            libspa::utils::Direction::Input,
            Some(node_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .expect("failed to connect PipeWire stream");
}

/// Grab a single frame from PipeWire. Used for the selection overlay screenshot.
fn grab_single_frame(
    node_id: u32,
    screen_width: u32,
    screen_height: u32,
) -> std::result::Result<CaptureFrame, CaptureError> {
    let (tx, rx) = mpsc::sync_channel::<CaptureFrame>(1);

    let handle = thread::spawn(move || {
        use pipewire as pw;
        use pipewire::properties::properties;

        let mainloop =
            pw::main_loop::MainLoopRc::new(None).expect("failed to create PipeWire main loop");
        let context = pw::context::ContextRc::new(&mainloop, None)
            .expect("failed to create PipeWire context");
        let core = context
            .connect_rc(None)
            .expect("failed to connect to PipeWire");

        let props = properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        };

        let stream = pw::stream::StreamRc::new(core, "rugif-screenshot", props)
            .expect("failed to create stream");

        let mainloop_quit = mainloop.clone();

        let _listener = stream
            .add_local_listener_with_user_data(false)
            .state_changed(|_stream, _user_data, _old, new| {
                tracing::debug!("PipeWire screenshot stream state: {new:?}");
            })
            .process(move |stream, got_frame| {
                if *got_frame {
                    return;
                }

                if let Some(mut buffer) = stream.dequeue_buffer() {
                    let datas = buffer.datas_mut();
                    if let Some(data) = datas.first_mut() {
                        let size = data.chunk().size() as usize;
                        if size > 0 {
                            if let Some(raw) = data.data() {
                                let usable = raw.len().min(size);
                                let frame =
                                    extract_full_frame(&raw[..usable], screen_width, screen_height);
                                let _ = tx.send(frame);
                                *got_frame = true;
                                mainloop_quit.quit();
                            }
                        }
                    }
                }
            })
            .register()
            .expect("failed to register PipeWire stream listener");

        connect_pw_stream(&stream, node_id, screen_width, screen_height);
        mainloop.run();
    });

    let frame = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| CaptureError::Screenshot("timed out waiting for PipeWire frame".into()))?;

    handle.join().ok();
    Ok(frame)
}

/// Extract a full-screen frame, converting BGRA to RGBA.
fn extract_full_frame(raw: &[u8], width: u32, height: u32) -> CaptureFrame {
    let expected = (width * height * 4) as usize;
    let mut data = Vec::with_capacity(expected);

    let usable = raw.len().min(expected);
    for pixel in raw[..usable].chunks_exact(4) {
        data.push(pixel[2]); // R
        data.push(pixel[1]); // G
        data.push(pixel[0]); // B
        data.push(pixel[3]); // A
    }

    data.resize(expected, 0);

    CaptureFrame {
        data,
        width,
        height,
        timestamp: Instant::now(),
    }
}

/// Capture frames continuously from PipeWire into a channel.
///
/// PipeWire only delivers frames on screen damage. To produce a consistent
/// framerate, we cache the latest raw buffer and re-send it at the target FPS
/// even when the screen is static.
fn run_pipewire_capture(
    node_id: u32,
    region: Region,
    fps: u8,
    screen_width: u32,
    screen_height: u32,
    tx: mpsc::SyncSender<CaptureFrame>,
    stop_flag: Arc<AtomicBool>,
) {
    use pipewire as pw;
    use pipewire::properties::properties;

    let mainloop =
        pw::main_loop::MainLoopRc::new(None).expect("failed to create PipeWire main loop");
    let context =
        pw::context::ContextRc::new(&mainloop, None).expect("failed to create PipeWire context");
    let core = context
        .connect_rc(None)
        .expect("failed to connect to PipeWire");

    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Video",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Screen",
    };

    let stream =
        pw::stream::StreamRc::new(core, "rugif-capture", props).expect("failed to create stream");

    // The process callback sends frames directly through a separate channel.
    // The main loop then forwards them to the encoder at the target FPS,
    // repeating the last frame when the screen is static.
    let (pw_tx, pw_rx) = mpsc::channel::<CaptureFrame>();

    let _listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(|_stream, _user_data, _old, new| {
            tracing::debug!("PipeWire stream state: {new:?}");
        })
        .process(move |stream, _user_data| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let size = data.chunk().size() as usize;
                    if size > 0 {
                        if let Some(raw) = data.data() {
                            let usable = raw.len().min(size);
                            let frame = crop_frame(&raw[..usable], screen_width, region);
                            let _ = pw_tx.send(frame);
                        }
                    }
                }
            }
        })
        .register()
        .expect("failed to register PipeWire stream listener");

    connect_pw_stream(&stream, node_id, screen_width, screen_height);

    // Frame forwarding loop on a separate thread: takes frames from PipeWire
    // and sends to the encoder at a steady FPS, repeating the last frame
    // when the screen is static.
    let stop_for_fwd = stop_flag.clone();
    let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);

    let fwd_handle = std::thread::spawn(move || {
        let mut last_frame: Option<CaptureFrame> = None;
        let mut frames_sent = 0u64;

        while !stop_for_fwd.load(Ordering::Relaxed) {
            // Drain any new frames from PipeWire (non-blocking).
            while let Ok(frame) = pw_rx.try_recv() {
                last_frame = Some(frame);
            }

            // Send a frame at the target interval.
            if let Some(ref frame) = last_frame {
                let mut send_frame = frame.clone();
                send_frame.timestamp = Instant::now();
                if tx.try_send(send_frame).is_err() {
                    break;
                }
                frames_sent += 1;
            }

            thread::sleep(frame_interval);
        }

        tracing::info!("PipeWire capture: sent {frames_sent} frames");
    });

    // Run PipeWire event loop until stopped.
    let loop_ = mainloop.loop_();
    while !stop_flag.load(Ordering::Relaxed) {
        loop_.iterate(Duration::from_millis(50));
    }

    fwd_handle.join().ok();
}

/// Crop a full-screen BGRA frame to the selected region, converting to RGBA.
fn crop_frame(raw: &[u8], screen_width: u32, region: Region) -> CaptureFrame {
    let stride = (screen_width * 4) as usize;
    let mut data = Vec::with_capacity((region.width * region.height * 4) as usize);

    for row in 0..region.height {
        let src_y = (region.y as u32 + row) as usize;
        let src_x = region.x as usize * 4;
        let start = src_y * stride + src_x;
        let end = start + (region.width as usize * 4);

        if end <= raw.len() {
            for pixel in raw[start..end].chunks_exact(4) {
                data.push(pixel[2]); // R
                data.push(pixel[1]); // G
                data.push(pixel[0]); // B
                data.push(pixel[3]); // A
            }
        }
    }

    CaptureFrame {
        data,
        width: region.width,
        height: region.height,
        timestamp: Instant::now(),
    }
}

impl ScreenCapture for WaylandCapture {
    fn screenshot_full(&mut self) -> Result<CaptureFrame, CaptureError> {
        tracing::info!("grabbing screenshot from PipeWire node {}", self.pw_node_id);
        grab_single_frame(self.pw_node_id, self.screen_width, self.screen_height)
            .map_err(|e| error_stack::report!(e))
    }

    fn start_capture(
        &mut self,
        region: Region,
        fps: u8,
    ) -> Result<CaptureReceiver, CaptureError> {
        if self.stop_flag.is_some() {
            return Err(error_stack::report!(CaptureError::AlreadyCapturing));
        }

        let (tx, rx) = mpsc::sync_channel::<CaptureFrame>(30);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let node_id = self.pw_node_id;
        let screen_w = self.screen_width;
        let screen_h = self.screen_height;

        let handle = thread::spawn(move || {
            run_pipewire_capture(node_id, region, fps, screen_w, screen_h, tx, stop_clone);
        });

        self.stop_flag = Some(stop_flag);
        self.capture_thread = Some(handle);

        Ok(rx)
    }

    fn stop_capture(&mut self) -> Result<(), CaptureError> {
        let stop_flag = self
            .stop_flag
            .take()
            .ok_or_else(|| error_stack::report!(CaptureError::NotCapturing))?;
        stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.capture_thread.take() {
            handle.join().ok();
        }

        Ok(())
    }
}
