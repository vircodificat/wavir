mod canvas;
mod state;
mod style;

use std::path::Path;
use std::sync::{Arc, Mutex};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;

use crate::canvas::{Render, Renderer, SvgRenderer};
use crate::state::{Signal, SignalType, State};
use crate::style::Style;

const FONT_PATH: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf";

const DASH_LEN: u32 = 12;
const DASH_GAP: u32 = 8;
const LANE_COLOR: Color = Color::RGB(180, 180, 190);
const TEXT_COLOR: Color = Color::RGB(230, 230, 235);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let style = Style::load("style.json")?;
    println!("style: {style:?}");
    println!("lane_height = {}", style.lane_height());

    // Headless screenshot mode: render one frame to an SVG file and exit,
    // without opening a window. Usage: waivir --svg [output.svg]
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--svg") {
        let path = args.get(i + 1).map(String::as_str).unwrap_or("frame.svg");
        let state = State::sample();
        let mut renderer = SvgRenderer::new(path, 1920, 1080);
        draw_frame(&mut renderer, &style, &Mutex::new(state))?;
        renderer.present()?;
        println!("wrote {path}");
        return Ok(());
    }

    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let timer = sdl.timer()?;

    let window = video
        .window("waivir", 1024, 640)
        .position_centered()
        .fullscreen_desktop()
        .build()?;

    let mut renderer = Renderer::new(window, Path::new(FONT_PATH))?;
    let mut event_pump = sdl.event_pump()?;
    let mut running = true;

    let state = Arc::new(Mutex::new(State::sample()));

    while running {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => running = false,
                // Move the "now" cursor by one jump interval, capped at the
                // start (0) and the end of the simulation.
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                } => {
                    let mut state = state.lock().unwrap();
                    state.now_ps = state.now_ps.saturating_sub(state.jump_interval_ps);
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                } => {
                    let mut state = state.lock().unwrap();
                    state.now_ps = (state.now_ps + state.jump_interval_ps).min(state.end_ps);
                }
                _ => {}
            }
        }

        draw_frame(&mut renderer, &style, &state)?;

        renderer.present()?;
        timer.delay(16); // ~60 FPS cap
    }

    Ok(())
}

fn draw_frame(r: &mut impl Render, style: &Style, state: &Mutex<State>) -> Result<(), String> {
    r.set_draw_color(Color::RGB(24, 24, 32));
    r.clear();

    let state = state.lock().unwrap();

    // Virtual-pixel scale: the viewport is inset by a uniform
    // `viewport_border` margin on all four sides and spans the remaining
    // height; its width follows from the screen's aspect ratio.
    let (screen_w, screen_h) = r.output_size()?;
    let border = style.viewport_border as f32;
    let scale = screen_h as f32 / (style.signal_viewport_height as f32 + 2.0 * border);
    let viewport_w = screen_w as f32 / scale;

    let border_px = (border * scale).round() as i32;

    // Border rectangle, inset `viewport_border` from all four screen edges.
    let bc = style.viewport_border_color;
    r.set_draw_color(Color::RGB(bc[0], bc[1], bc[2]));
    let w = screen_w as i32 - 2 * border_px;
    let h = screen_h as i32 - 2 * border_px;
    r.rect(border_px, border_px, w, h)?;

    draw_time_ruler(r, style, &state, scale, viewport_w, border)?;

    for (i, signal) in state.signals.iter().enumerate() {
        draw_lane(r, style, &state, signal, i, scale, viewport_w, border)?;
    }

    // "Now" cursor: a vertical line spanning the viewport at the current
    // simulation time, drawn on top of the lanes and border.
    let x_now = (time_to_x(state.now_ps, &state, style, viewport_w, border) * scale).round() as i32;
    let c = style.now_cursor_color;
    r.set_draw_color(Color::RGB(c[0], c[1], c[2]));
    r.line(x_now, border_px, x_now, screen_h as i32 - border_px)?;

    Ok(())
}

/// Draw the time ruler's tick marks at the top of the screen: one tick at
/// every multiple of `jump_interval_ps` in the viewport, extending from the
/// top of the screen down by `time_ruler_tick_height`.
fn draw_time_ruler(
    r: &mut impl Render,
    style: &Style,
    state: &State,
    scale: f32,
    viewport_w: f32,
    border: f32,
) -> Result<(), String> {
    let sy = |v: f32| ((border + v) * scale).round() as i32;
    let tick_bottom = sy(style.time_ruler_tick_height as f32);

    // First tick at or after the start of the viewport, then every
    // `jump_interval_ps` until the end of the viewport.
    let first_tick = state
        .viewport_start_ps
        .div_ceil(state.jump_interval_ps)
        * state.jump_interval_ps;
    r.set_draw_color(LANE_COLOR);
    let mut t_ps = first_tick;
    while t_ps <= state.viewport_end_ps {
        let x = (time_to_x(t_ps, state, style, viewport_w, border) * scale).round() as i32;
        r.line(x, sy(0.0), x, tick_bottom)?;
        t_ps += state.jump_interval_ps;
    }

    Ok(())
}

/// Interpolate a timestamp to an x position, in virtual pixels:
/// `viewport_start_ps` maps to the left edge of the waveform area
/// (`border + name_width`), `viewport_end_ps` maps to the right margin edge
/// (`viewport_w - border`).
fn time_to_x(t_ps: u64, state: &State, style: &Style, viewport_w: f32, border: f32) -> f32 {
    let x_left = border + style.name_width as f32;
    let x_right = viewport_w - border;
    let t_start = state.viewport_start_ps as f32;
    let t_end = state.viewport_end_ps as f32;
    let frac = ((t_ps as f32 - t_start) / (t_end - t_start)).clamp(0.0, 1.0);
    x_left + frac * (x_right - x_left)
}

fn draw_lane(
    r: &mut impl Render,
    style: &Style,
    state: &State,
    signal: &Signal,
    i: usize,
    scale: f32,
    viewport_w: f32,
    border: f32,
) -> Result<(), String> {
    let lane_height = style.lane_height();
    // Everything is shifted down by the time ruler's height to make space
    // for it at the top of the screen.
    let y_top = style.time_ruler_height + (i as u32) * (lane_height + style.gap);
    let y_bottom = style.time_ruler_height + (i + 1) as u32 * (lane_height + style.gap) - style.gap;

    // Virtual pixels -> screen pixels. Y coordinates are shifted down by the
    // border; X coordinates are passed in border-inclusive (see `time_to_x`).
    let sy = |v: f32| ((border + v) * scale).round() as i32;
    let sx = |v: f32| (v * scale).round() as i32;
    let dash_len = (DASH_LEN as f32 * scale).round().max(1.0) as u32;
    let dash_gap = (DASH_GAP as f32 * scale).round().max(1.0) as u32;

    let x_v = border + style.name_width as f32;
    let x_left = border;
    let x_right = viewport_w - border;

    // Lane frame.
    r.set_draw_color(LANE_COLOR);
    r.dashed_line(
        sx(x_v),
        sy(y_top as f32),
        sx(x_v),
        sy((y_top + lane_height) as f32),
        dash_len,
        dash_gap,
    )?;
    r.dashed_line(
        sx(x_left),
        sy(y_top as f32),
        sx(x_right),
        sy(y_top as f32),
        dash_len,
        dash_gap,
    )?;
    r.dashed_line(
        sx(x_left),
        sy(y_bottom as f32),
        sx(x_right),
        sy(y_bottom as f32),
        dash_len,
        dash_gap,
    )?;

    // Signal name, vertically centered in the name area (x < name_width),
    // label_inset left-padded. Skipped if it doesn't fit.
    let font_px = style.label_fontsize as f32 * scale;
    let (tw, th) = r.text_size(&signal.name, font_px);
    let inset_px = (style.label_inset as f32 * scale).round() as i32;
    let label_x = sx(border) + inset_px;
    let lane_mid = y_top as f32 + lane_height as f32 / 2.0;
    if label_x + tw as i32 <= sx(x_v) {
        r.text(
            &signal.name,
            label_x,
            sy(lane_mid) - th as i32 / 2,
            font_px,
            TEXT_COLOR,
        )?;
    }

    // Waveform geometry, in virtual pixels: the signal levels are inset one
    // rail from the top and bottom of the lane.
    let hi = (y_top + style.rail) as f32;
    let lo = (y_top + lane_height - style.rail) as f32;

    // Time -> x mapping, shared with the "now" cursor: t = viewport_start_ps
    // -> x = border + name_width, t = viewport_end_ps -> x = right margin edge.
    let x_of = |t_ps: u64| time_to_x(t_ps, state, style, viewport_w, border);

    let sc = style.signal_color;
    r.set_draw_color(Color::RGB(sc[0], sc[1], sc[2]));

    match signal.signal_type {
        SignalType::Bit => {
            let mut prev: Option<u64> = None;
            let mut cur_y: Option<f32> = None;
            let mut last_x = x_of(state.viewport_start_ps);

            for ev in &signal.events {
                let v = ev.new_value.0;
                let x = x_of(ev.time_ps);
                let y = if v != 0 { hi } else { lo };

                if let (Some(py), Some(pv)) = (cur_y, prev) {
                    // Horizontal at the previous level up to the event time.
                    r.line(sx(last_x), sy(py), sx(x), sy(py))?;
                    // Vertical transition, if the value changed.
                    if pv != v {
                        r.line(sx(x), sy(py), sx(x), sy(y))?;
                    }
                }

                prev = Some(v);
                cur_y = Some(y);
                last_x = x;
            }

            // Extend the final level to the right margin edge.
            if let Some(py) = cur_y {
                r.line(sx(last_x), sy(py), sx(x_right), sy(py))?;
            }
        }
        SignalType::Value => {
            let mut seg_start = x_of(state.viewport_start_ps);
            let mut seg_val: Option<u64> = None;

            for ev in &signal.events {
                let x = x_of(ev.time_ps);
                let v = ev.new_value.0;

                if let Some(pv) = seg_val {
                    draw_bus_segment(r, style, pv, seg_start, x, scale, border, hi, lo)?;
                    // Transition tick spanning the full bus height.
                    r.line(sx(x), sy(hi), sx(x), sy(lo))?;
                }

                seg_val = Some(v);
                seg_start = x;
            }

            if let Some(pv) = seg_val {
                draw_bus_segment(r, style, pv, seg_start, x_right, scale, border, hi, lo)?;
            }
        }
    }

    Ok(())
}

/// Draw one bus segment: top and bottom rails with the hex value label
/// in between, `label_inset` left-padded from the segment's left edge
/// (only if the label fits within the segment).
fn draw_bus_segment(
    r: &mut impl Render,
    style: &Style,
    value: u64,
    x_a: f32,
    x_b: f32,
    scale: f32,
    border: f32,
    hi: f32,
    lo: f32,
) -> Result<(), String> {
    let sx = |v: f32| (v * scale).round() as i32;
    // Y coordinates are lane-virtual pixels, shifted down by the border like
    // everything else.
    let sy = |v: f32| ((border + v) * scale).round() as i32;
    let mid = (hi + lo) / 2.0;

    let (xa, xb) = (sx(x_a), sx(x_b));
    r.line(xa, sy(hi), xb, sy(hi))?;
    r.line(xa, sy(lo), xb, sy(lo))?;

    let text = format!("0x{value:x}");
    let font_px = style.label_fontsize as f32 * scale;
    let (tw, th) = r.text_size(&text, font_px);
    let inset_px = (style.label_inset as f32 * scale).round() as i32;
    if xa + inset_px + tw as i32 <= xb {
        r.text(&text, xa + inset_px, sy(mid) - th as i32 / 2, font_px, TEXT_COLOR)?;
    }

    Ok(())
}
