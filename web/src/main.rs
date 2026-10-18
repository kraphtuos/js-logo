//! Browser front end: scan ringcodes from the camera or images, and create new ones.

mod dial;

use std::cell::RefCell;
use std::f64::consts::TAU;
use std::rc::Rc;

use js_sys::{Object, Reflect};
use ringcode::{Detection, Ellipse, SvgOptions};
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    CanvasRenderingContext2d, ClipboardEvent, Document, DragEvent, Event, EventTarget, File, HtmlAnchorElement,
    HtmlButtonElement, HtmlCanvasElement, HtmlElement, HtmlImageElement, HtmlInputElement, HtmlVideoElement, ImageData,
    MediaStream, MediaStreamConstraints, MediaStreamTrack, Url,
};

/// Camera frames are scaled to at most this size (long side) before decoding.
const FRAME_DECODE_SIZE: f64 = 640.0;
/// Still images are displayed at most this large (long side).
const STILL_DISPLAY_SIZE: f64 = 1600.0;
/// Overlay colour; matches `--accent` in style.css.
const ACCENT: &str = "#00a676";

fn main() {
    console_error_panic_hook::set_once();
    if let Err(e) = start() {
        web_sys::console::error_1(&e);
    }
}

fn document() -> Document {
    web_sys::window().expect("window").document().expect("document")
}

fn by_id<T: JsCast>(id: &str) -> Result<T, JsValue> {
    document()
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("missing #{id}")))?
        .dyn_into::<T>()
        .map_err(|_| JsValue::from_str(&format!("#{id} has an unexpected element type")))
}

fn context_2d(canvas: &HtmlCanvasElement) -> Result<CanvasRenderingContext2d, JsValue> {
    // Pixels are read back every frame; this keeps the canvas in CPU memory.
    let opts = Object::new();
    Reflect::set(&opts, &"willReadFrequently".into(), &JsValue::TRUE)?;
    canvas
        .get_context_with_context_options("2d", &opts)?
        .ok_or("no 2d context")?
        .dyn_into()
        .map_err(|_| "no 2d context".into())
}

/// Adds an event listener that lives for the rest of the page.
fn on<E: FromWasmAbi + 'static>(target: &EventTarget, event: &str, handler: impl FnMut(E) + 'static) {
    let closure = Closure::<dyn FnMut(E)>::new(handler);
    target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref()).expect("add listener");
    closure.forget();
}

/// Best-effort readable message from a thrown JS value.
fn describe(err: &JsValue) -> String {
    let field = |name: &str| Reflect::get(err, &name.into()).ok().and_then(|v| v.as_string()).filter(|s| !s.is_empty());
    match (field("name"), field("message")) {
        (Some(name), Some(message)) => format!("{name}: {message}"),
        (_, Some(message)) => message,
        _ => err.as_string().unwrap_or_else(|| "unknown error".into()),
    }
}

struct Elements {
    tab_scan: HtmlElement,
    tab_create: HtmlElement,
    view_scan: HtmlElement,
    view_create: HtmlElement,
    stage: HtmlElement,
    placeholder: HtmlElement,
    screen: HtmlCanvasElement,
    video: HtmlVideoElement,
    camera: HtmlButtonElement,
    file: HtmlInputElement,
    result_text: HtmlElement,
    result_detail: HtmlElement,
    word: HtmlInputElement,
    word_error: HtmlElement,
    show_dial: HtmlInputElement,
    preview: HtmlElement,
    download_svg: HtmlButtonElement,
    download_png: HtmlButtonElement,
    test_scan: HtmlButtonElement,
}

struct App {
    el: Elements,
    screen_ctx: CanvasRenderingContext2d,
    /// Off-screen canvas that camera frames are scaled into for decoding.
    work: HtmlCanvasElement,
    work_ctx: CanvasRenderingContext2d,
    stream: Option<MediaStream>,
    /// Incremented per camera start so a stale frame loop can tell it should stop.
    camera_session: u32,
    last_text: Option<String>,
}

type Shared = Rc<RefCell<App>>;

fn start() -> Result<(), JsValue> {
    let el = Elements {
        tab_scan: by_id("tab-scan")?,
        tab_create: by_id("tab-create")?,
        view_scan: by_id("view-scan")?,
        view_create: by_id("view-create")?,
        stage: by_id("stage")?,
        placeholder: by_id("placeholder")?,
        screen: by_id("screen")?,
        video: by_id("video")?,
        camera: by_id("camera")?,
        file: by_id("file")?,
        result_text: by_id("result-text")?,
        result_detail: by_id("result-detail")?,
        word: by_id("word")?,
        word_error: by_id("word-error")?,
        show_dial: by_id("show-dial")?,
        preview: by_id("preview")?,
        download_svg: by_id("download-svg")?,
        download_png: by_id("download-png")?,
        test_scan: by_id("test-scan")?,
    };
    let mark_options = SvgOptions { foreground: "currentColor".into(), background: None, margin: 0.0 };
    by_id::<HtmlElement>("brand-mark")?.set_inner_html(&ringcode::svg("RC", &mark_options).expect("valid word"));
    by_id::<HtmlElement>("about-dial")?.set_inner_html(&dial::svg("JSC"));

    let screen_ctx = context_2d(&el.screen)?;
    let work: HtmlCanvasElement = document().create_element("canvas")?.dyn_into()?;
    let work_ctx = context_2d(&work)?;
    let app =
        Rc::new(RefCell::new(App { el, screen_ctx, work, work_ctx, stream: None, camera_session: 0, last_text: None }));
    wire_events(&app);
    app.borrow().update_preview();
    Ok(())
}

fn wire_events(app: &Shared) {
    let a = app.borrow();
    let el = &a.el;

    let shared = app.clone();
    on(&el.tab_scan, "click", move |_: Event| select_tab(&shared, true));
    let shared = app.clone();
    on(&el.tab_create, "click", move |_: Event| select_tab(&shared, false));

    let shared = app.clone();
    on(&el.camera, "click", move |_: Event| {
        if shared.borrow().stream.is_some() {
            stop_camera(&shared);
            return;
        }
        let shared = shared.clone();
        spawn_local(async move {
            if let Err(e) = start_camera(&shared).await {
                shared.borrow().show_message("Camera unavailable", &describe(&e));
            }
        });
    });

    let shared = app.clone();
    on(&el.file, "change", move |_: Event| {
        let input = shared.borrow().el.file.clone();
        if let Some(file) = input.files().and_then(|files| files.get(0)) {
            load_file(&shared, file);
        }
        // Allow choosing the same file again.
        input.set_value("");
    });

    on(&el.stage, "dragover", |e: DragEvent| e.prevent_default());
    let shared = app.clone();
    on(&el.stage, "drop", move |e: DragEvent| {
        e.prevent_default();
        if let Some(file) = e.data_transfer().and_then(|dt| dt.files()).and_then(|files| files.get(0)) {
            select_tab(&shared, true);
            load_file(&shared, file);
        }
    });
    let shared = app.clone();
    on(&document(), "paste", move |e: ClipboardEvent| {
        if let Some(file) = e.clipboard_data().and_then(|dt| dt.files()).and_then(|files| files.get(0)) {
            select_tab(&shared, true);
            load_file(&shared, file);
        }
    });

    let shared = app.clone();
    on(&el.word, "input", move |_: Event| shared.borrow().update_preview());
    let shared = app.clone();
    on(&el.show_dial, "change", move |_: Event| shared.borrow().update_preview());

    let shared = app.clone();
    on(&el.download_svg, "click", move |_: Event| {
        let word = shared.borrow().word();
        if let Ok(svg) = ringcode::svg(&word, &SvgOptions::default()) {
            let url = format!("data:image/svg+xml;charset=utf-8,{}", js_sys::encode_uri_component(&svg));
            download(&url, &format!("ringcode-{word}.svg"));
        }
    });
    let shared = app.clone();
    on(&el.download_png, "click", move |_: Event| {
        let word = shared.borrow().word();
        if let Err(e) = download_png(&word) {
            shared.borrow().show_message("Could not export PNG", &describe(&e));
        }
    });
    let shared = app.clone();
    on(&el.test_scan, "click", move |_: Event| {
        let word = shared.borrow().word();
        let Ok(img) = ringcode::render_word(&word, 720, 0.4) else { return };
        select_tab(&shared, true);
        stop_camera(&shared);
        let rgba = img.to_rgba();
        let (w, h) = (img.width as f64, img.height as f64);
        shared.borrow_mut().scan_still(w, h, |ctx, _, _| {
            let data = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w as u32, h as u32)?;
            ctx.put_image_data(&data, 0.0, 0.0)
        });
    });
}

fn select_tab(app: &Shared, scan: bool) {
    {
        let a = app.borrow();
        let _ = a.el.tab_scan.set_attribute("aria-selected", if scan { "true" } else { "false" });
        let _ = a.el.tab_create.set_attribute("aria-selected", if scan { "false" } else { "true" });
        a.el.view_scan.set_hidden(!scan);
        a.el.view_create.set_hidden(scan);
    }
    if !scan {
        stop_camera(app);
    }
}

async fn start_camera(app: &Shared) -> Result<(), JsValue> {
    let devices = web_sys::window().expect("window").navigator().media_devices()?;
    let ideal = |v: u32| -> Result<Object, JsValue> {
        let o = Object::new();
        Reflect::set(&o, &"ideal".into(), &v.into())?;
        Ok(o)
    };
    let video = Object::new();
    Reflect::set(&video, &"facingMode".into(), &"environment".into())?;
    Reflect::set(&video, &"width".into(), &ideal(1280)?.into())?;
    Reflect::set(&video, &"height".into(), &ideal(720)?.into())?;
    let constraints = MediaStreamConstraints::new();
    constraints.set_video(&video);
    constraints.set_audio(&JsValue::FALSE);

    let camera = app.borrow().el.camera.clone();
    camera.set_disabled(true);
    let result = match devices.get_user_media_with_constraints(&constraints) {
        Ok(promise) => JsFuture::from(promise).await,
        Err(e) => Err(e),
    };
    camera.set_disabled(false);
    let stream: MediaStream = result?.dyn_into()?;

    let video_el = app.borrow().el.video.clone();
    video_el.set_src_object(Some(&stream));
    JsFuture::from(video_el.play()?).await?;

    let session = {
        let mut a = app.borrow_mut();
        a.stream = Some(stream);
        a.camera_session += 1;
        a.el.camera.set_text_content(Some("Stop camera"));
        a.el.placeholder.set_hidden(true);
        if a.last_text.is_none() {
            a.set_result("—", "Looking for a ringcode…");
        }
        a.camera_session
    };
    run_frame_loop(app.clone(), session);
    Ok(())
}

fn stop_camera(app: &Shared) {
    let mut a = app.borrow_mut();
    if let Some(stream) = a.stream.take() {
        for track in stream.get_tracks().iter() {
            track.unchecked_into::<MediaStreamTrack>().stop();
        }
        a.el.video.set_src_object(None);
        a.el.camera.set_text_content(Some("Start camera"));
    }
}

type FrameCallback = Closure<dyn FnMut()>;

fn request_frame(f: &FrameCallback) {
    web_sys::window().expect("window").request_animation_frame(f.as_ref().unchecked_ref()).expect("rAF");
}

/// Decodes one camera frame per animation frame until the camera session ends.
fn run_frame_loop(app: Shared, session: u32) {
    let tick: Rc<RefCell<Option<FrameCallback>>> = Rc::new(RefCell::new(None));
    let next = tick.clone();
    *tick.borrow_mut() = Some(Closure::new(move || {
        let live = {
            let a = app.borrow();
            a.stream.is_some() && a.camera_session == session
        };
        if !live {
            // Drop the closure (and with it this loop).
            next.borrow_mut().take();
            return;
        }
        if let Err(e) = app.borrow_mut().scan_video_frame() {
            web_sys::console::warn_1(&e);
        }
        request_frame(next.borrow().as_ref().expect("loop closure"));
    }));
    request_frame(tick.borrow().as_ref().expect("loop closure"));
}

fn load_file(app: &Shared, file: File) {
    let url = match Url::create_object_url_with_blob(&file) {
        Ok(url) => url,
        Err(e) => return app.borrow().show_message("Could not open file", &describe(&e)),
    };
    let img = match HtmlImageElement::new() {
        Ok(img) => img,
        Err(e) => return app.borrow().show_message("Could not open file", &describe(&e)),
    };
    let (shared, image, src) = (app.clone(), img.clone(), url.clone());
    let onload = Closure::once_into_js(move || {
        stop_camera(&shared);
        let (w, h) = (image.natural_width() as f64, image.natural_height() as f64);
        shared.borrow_mut().scan_still(w, h, |ctx, dw, dh| {
            ctx.draw_image_with_html_image_element_and_dw_and_dh(&image, 0.0, 0.0, dw, dh)
        });
        let _ = Url::revoke_object_url(&src);
    });
    let (shared, src) = (app.clone(), url.clone());
    let onerror = Closure::once_into_js(move || {
        shared.borrow().show_message("Could not read that image", "Try a PNG or JPEG file.");
        let _ = Url::revoke_object_url(&src);
    });
    img.set_onload(Some(onload.unchecked_ref()));
    img.set_onerror(Some(onerror.unchecked_ref()));
    img.set_src(&url);
}

fn download(url: &str, filename: &str) {
    if let Ok(a) = document().create_element("a").map(|a| a.unchecked_into::<HtmlAnchorElement>()) {
        a.set_href(url);
        a.set_download(filename);
        a.click();
    }
}

fn download_png(word: &str) -> Result<(), JsValue> {
    let img = ringcode::render_word(word, 1024, 0.2).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let canvas: HtmlCanvasElement = document().create_element("canvas")?.dyn_into()?;
    canvas.set_width(img.width as u32);
    canvas.set_height(img.height as u32);
    let ctx = context_2d(&canvas)?;
    let rgba = img.to_rgba();
    ctx.put_image_data(
        &ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), img.width as u32, img.height as u32)?,
        0.0,
        0.0,
    )?;
    download(&canvas.to_data_url()?, &format!("ringcode-{word}.png"));
    Ok(())
}

impl App {
    fn set_result(&self, text: &str, detail: &str) {
        self.el.result_text.set_text_content(Some(text));
        self.el.result_detail.set_text_content(Some(detail));
    }

    fn show_message(&self, title: &str, detail: &str) {
        self.set_result("—", &format!("{title}. {detail}"));
    }

    fn scan_video_frame(&mut self) -> Result<(), JsValue> {
        let video = &self.el.video;
        let (vw, vh) = (video.video_width(), video.video_height());
        if vw == 0 || vh == 0 {
            return Ok(());
        }
        if self.el.screen.width() != vw || self.el.screen.height() != vh {
            self.el.screen.set_width(vw);
            self.el.screen.set_height(vh);
        }
        self.screen_ctx.draw_image_with_html_video_element(video, 0.0, 0.0)?;

        let scale = (FRAME_DECODE_SIZE / vw.max(vh) as f64).min(1.0);
        let (pw, ph) = ((vw as f64 * scale).round() as u32, (vh as f64 * scale).round() as u32);
        if self.work.width() != pw || self.work.height() != ph {
            self.work.set_width(pw);
            self.work.set_height(ph);
        }
        self.work_ctx.draw_image_with_html_video_element_and_dw_and_dh(video, 0.0, 0.0, pw as f64, ph as f64)?;
        let pixels = self.work_ctx.get_image_data(0.0, 0.0, pw as f64, ph as f64)?.data();
        let found = ringcode::decode_rgba(&pixels, pw as usize, ph as usize);
        draw_overlay(&self.screen_ctx, &found, 1.0 / scale);
        self.report(&found, true);
        Ok(())
    }

    /// Shows a still image on the stage and decodes it. `draw` paints the
    /// image into the context at the given display size.
    fn scan_still(
        &mut self,
        width: f64,
        height: f64,
        draw: impl FnOnce(&CanvasRenderingContext2d, f64, f64) -> Result<(), JsValue>,
    ) {
        let scale = (STILL_DISPLAY_SIZE / width.max(height)).min(1.0);
        let (dw, dh) = ((width * scale).round().max(1.0), (height * scale).round().max(1.0));
        self.el.screen.set_width(dw as u32);
        self.el.screen.set_height(dh as u32);
        let ctx = &self.screen_ctx;
        // Transparent images read as dark-on-white.
        ctx.set_fill_style_str("#fff");
        ctx.fill_rect(0.0, 0.0, dw, dh);
        if let Err(e) = draw(ctx, dw, dh) {
            return self.show_message("Could not draw image", &describe(&e));
        }
        self.el.placeholder.set_hidden(true);
        let pixels = match ctx.get_image_data(0.0, 0.0, dw, dh) {
            Ok(data) => data.data(),
            Err(e) => return self.show_message("Could not read image", &describe(&e)),
        };
        let found = ringcode::decode_rgba(&pixels, dw as usize, dh as usize);
        draw_overlay(ctx, &found, 1.0);
        self.report(&found, false);
    }

    fn report(&mut self, found: &[Detection], live: bool) {
        if found.is_empty() {
            if !live {
                self.last_text = None;
                self.set_result("—", "No ringcode found. Codes need to be roughly upright and in focus.");
            }
            // While scanning live, keep showing the last code read.
            return;
        }
        let text = found.iter().map(|d| d.text.as_str()).collect::<Vec<_>>().join(" · ");
        let d = &found[0];
        let rotation = match d.rotation_deg {
            r if r.abs() < 1.0 => "upright".to_string(),
            r if r > 0.0 => format!("turned right {r:.0}°"),
            r => format!("turned left {:.0}°", -r),
        };
        let mut detail = format!("{} rings · {rotation}", d.rings.len());
        if d.perspective {
            detail.push_str(" · perspective corrected");
        }
        detail.push_str(if d.max_residual_deg < 3.0 { " · clean read" } else { " · marginal read" });
        if found.len() > 1 {
            detail = format!("{} codes · first: {detail}", found.len());
        }
        if live && self.last_text.as_deref() != Some(text.as_str()) {
            let _ = web_sys::window().expect("window").navigator().vibrate_with_duration(40);
        }
        self.set_result(&text, &detail);
        self.last_text = Some(text);
    }

    /// The word typed in the Create tab, cleaned up to letters only.
    fn word(&self) -> String {
        self.el
            .word
            .value()
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_uppercase())
            .take(ringcode::spec::MAX_RINGS)
            .collect()
    }

    fn update_preview(&self) {
        let word = self.word();
        if self.el.word.value() != word {
            self.el.word.set_value(&word);
        }
        let valid = ringcode::parse_word(&word);
        for button in [&self.el.download_svg, &self.el.download_png, &self.el.test_scan] {
            button.set_disabled(valid.is_err());
        }
        match valid {
            Ok(_) => {
                self.el.word_error.set_hidden(true);
                let svg = if self.el.show_dial.checked() {
                    dial::svg(&word)
                } else {
                    ringcode::svg(&word, &SvgOptions::default()).expect("valid word")
                };
                self.el.preview.set_inner_html(&svg);
            }
            Err(e) => {
                self.el.word_error.set_text_content(Some(&format!("Can't encode that: {e}.")));
                self.el.word_error.set_hidden(false);
            }
        }
    }
}

/// Outlines each code's rings, marks each gap with its letter and labels the code.
fn draw_overlay(ctx: &CanvasRenderingContext2d, found: &[Detection], scale: f64) {
    for d in found {
        let o = d.outer();
        let outer = Ellipse { cx: o.cx * scale, cy: o.cy * scale, a: o.a * scale, b: o.b * scale, angle: o.angle };
        let r = outer.radius();
        let line = (r / 45.0).max(2.0);
        ctx.set_line_width(line);
        ctx.set_stroke_style_str(ACCENT);
        for ring in &d.rings {
            let e = ring.ellipse;
            ctx.begin_path();
            let _ = ctx.ellipse(e.cx * scale, e.cy * scale, e.a * scale, e.b * scale, e.angle, 0.0, TAU);
            ctx.stroke();
        }

        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        let bubble = (r * 0.08).max(10.0);
        ctx.set_font(&format!("700 {:.0}px system-ui, sans-serif", bubble * 1.3));
        for ring in &d.rings {
            let (x, y) = (ring.gap_point.0 * scale, ring.gap_point.1 * scale);
            ctx.begin_path();
            let _ = ctx.arc(x, y, bubble, 0.0, TAU);
            ctx.set_fill_style_str(ACCENT);
            ctx.fill();
            ctx.set_fill_style_str("#fff");
            let _ = ctx.fill_text(&ring.letter.to_string(), x, y + bubble * 0.08);
        }

        // Label just above the code.
        let size = (r * 0.2).max(16.0);
        ctx.set_font(&format!("700 {size:.0}px ui-monospace, SFMono-Regular, Menlo, monospace"));
        let width = ctx.measure_text(&d.text).map(|m| m.width()).unwrap_or(size * d.text.len() as f64);
        let (pad_x, pad_y) = (size * 0.45, size * 0.3);
        let half_height = top_extent(&outer);
        let (lx, ly) = (outer.cx, (outer.cy - half_height - line * 3.0 - size / 2.0 - pad_y).max(size / 2.0 + pad_y));
        ctx.set_fill_style_str(ACCENT);
        ctx.begin_path();
        let _ = ctx.round_rect_with_f64(
            lx - width / 2.0 - pad_x,
            ly - size / 2.0 - pad_y,
            width + 2.0 * pad_x,
            size + 2.0 * pad_y,
            size * 0.35,
        );
        ctx.fill();
        ctx.set_fill_style_str("#fff");
        let _ = ctx.fill_text(&d.text, lx, ly + size * 0.05);
    }
}

/// Vertical half-extent of an ellipse.
fn top_extent(e: &Ellipse) -> f64 {
    let (s, c) = e.angle.sin_cos();
    ((e.a * s).powi(2) + (e.b * c).powi(2)).sqrt()
}
