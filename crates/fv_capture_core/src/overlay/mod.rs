use std::collections::HashMap;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::input::{InputEvent, InputEventKind, KeyCode, MouseButton};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WheelDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OverlayEventKind {
    KeyCombo(Vec<KeyCode>),
    MouseClick { button: MouseButton, x: f64, y: f64 },
    MouseDoubleClick { button: MouseButton, x: f64, y: f64 },
    MouseDrag { start: Point, end: Point },
    MouseWheel { direction: WheelDirection },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayEvent {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub kind: OverlayEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LabelPosition {
    BottomCenter,
    TopCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayLabelFont {
    Compact,
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl OverlayColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub show_keyboard: bool,
    pub show_mouse: bool,
    pub show_cursor: bool,
    pub label_scale: f32,
    pub label_font: OverlayLabelFont,
    pub label_position: LabelPosition,
    pub display_ms: u64,
    pub opacity: f32,
    pub keyboard_background: OverlayColor,
    pub keyboard_text: OverlayColor,
    pub keyboard_border: OverlayColor,
    pub mouse_primary: OverlayColor,
    pub mouse_secondary: OverlayColor,
    pub cursor_color: OverlayColor,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            show_keyboard: true,
            show_mouse: true,
            show_cursor: false,
            label_scale: 1.0,
            label_font: OverlayLabelFont::Bold,
            label_position: LabelPosition::BottomCenter,
            display_ms: 1_200,
            opacity: 0.92,
            keyboard_background: OverlayColor::new(15, 20, 26),
            keyboard_text: OverlayColor::new(246, 250, 255),
            keyboard_border: OverlayColor::new(235, 241, 247),
            mouse_primary: OverlayColor::new(57, 255, 210),
            mouse_secondary: OverlayColor::new(255, 255, 255),
            cursor_color: OverlayColor::new(255, 245, 140),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OverlayTimeline {
    pub events: Vec<OverlayEvent>,
    pub mouse_positions: Vec<(u64, Point)>,
}

impl OverlayTimeline {
    pub fn from_input_events(input_events: &[InputEvent], settings: &OverlaySettings) -> Self {
        let mut sorted = input_events.to_vec();
        sorted.sort_by_key(|event| event.timestamp_ms);

        let mut down_keys: Vec<KeyCode> = Vec::new();
        let mut mouse_down: HashMap<MouseButton, (u64, Point)> = HashMap::new();
        let mut last_mouse_position = Point { x: 0.0, y: 0.0 };
        let mut last_click: Option<(MouseButton, u64, Point)> = None;
        let mut events = Vec::new();
        let mut mouse_positions = Vec::new();

        for event in sorted {
            match event.kind {
                InputEventKind::KeyDown(key) => {
                    if !down_keys.contains(&key) {
                        down_keys.push(key.clone());
                    }

                    if settings.show_keyboard && !key.is_modifier() {
                        let mut combo = ordered_modifiers(&down_keys);
                        combo.push(key);
                        events.push(OverlayEvent {
                            start_ms: event.timestamp_ms,
                            duration_ms: settings.display_ms,
                            kind: OverlayEventKind::KeyCombo(combo),
                        });
                    }
                }
                InputEventKind::KeyUp(key) => {
                    down_keys.retain(|held| held != &key);
                }
                InputEventKind::MouseMove { x, y } => {
                    last_mouse_position = Point { x, y };
                    mouse_positions.push((event.timestamp_ms, last_mouse_position));
                }
                InputEventKind::MouseDown(button) => {
                    mouse_down.insert(button, (event.timestamp_ms, last_mouse_position));
                }
                InputEventKind::MouseUp(button) if settings.show_mouse => {
                    let Some((started_ms, started_at)) = mouse_down.remove(&button) else {
                        continue;
                    };
                    let distance = point_distance(started_at, last_mouse_position);
                    if distance > 8.0 && event.timestamp_ms.saturating_sub(started_ms) > 100 {
                        events.push(OverlayEvent {
                            start_ms: started_ms,
                            duration_ms: event.timestamp_ms.saturating_sub(started_ms).max(200),
                            kind: OverlayEventKind::MouseDrag {
                                start: started_at,
                                end: last_mouse_position,
                            },
                        });
                    } else {
                        let is_double = last_click
                            .as_ref()
                            .map(|(last_button, last_ms, last_pos)| {
                                *last_button == button
                                    && event.timestamp_ms.saturating_sub(*last_ms) <= 420
                                    && point_distance(*last_pos, last_mouse_position) <= 8.0
                            })
                            .unwrap_or(false);

                        let kind = if is_double {
                            OverlayEventKind::MouseDoubleClick {
                                button,
                                x: last_mouse_position.x,
                                y: last_mouse_position.y,
                            }
                        } else {
                            OverlayEventKind::MouseClick {
                                button,
                                x: last_mouse_position.x,
                                y: last_mouse_position.y,
                            }
                        };

                        events.push(OverlayEvent {
                            start_ms: event.timestamp_ms,
                            duration_ms: settings.display_ms.min(800),
                            kind,
                        });
                        last_click = Some((button, event.timestamp_ms, last_mouse_position));
                    }
                }
                InputEventKind::MouseUp(_) => {}
                InputEventKind::MouseWheel { delta_x, delta_y } if settings.show_mouse => {
                    let direction = if delta_y.abs() >= delta_x.abs() {
                        if delta_y >= 0.0 {
                            WheelDirection::Up
                        } else {
                            WheelDirection::Down
                        }
                    } else if delta_x >= 0.0 {
                        WheelDirection::Right
                    } else {
                        WheelDirection::Left
                    };

                    events.push(OverlayEvent {
                        start_ms: event.timestamp_ms,
                        duration_ms: settings.display_ms.min(800),
                        kind: OverlayEventKind::MouseWheel { direction },
                    });
                }
                InputEventKind::MouseWheel { .. } => {}
            }
        }

        Self {
            events,
            mouse_positions,
        }
    }

    pub fn active_events(&self, timestamp_ms: u64) -> impl Iterator<Item = &OverlayEvent> {
        self.events.iter().filter(move |event| {
            timestamp_ms >= event.start_ms
                && timestamp_ms <= event.start_ms.saturating_add(event.duration_ms)
        })
    }

    pub fn mouse_position_at(&self, timestamp_ms: u64) -> Option<Point> {
        self.mouse_positions
            .iter()
            .rev()
            .find(|(position_ms, _)| *position_ms <= timestamp_ms)
            .map(|(_, point)| *point)
    }
}

pub fn composite_frame(
    frame: &mut RgbaImage,
    timeline: &OverlayTimeline,
    timestamp_ms: u64,
    settings: &OverlaySettings,
) {
    let active_events: Vec<_> = timeline.active_events(timestamp_ms).collect();
    for event in &active_events {
        match &event.kind {
            OverlayEventKind::MouseClick { x, y, .. }
            | OverlayEventKind::MouseDoubleClick { x, y, .. } => {
                if settings.show_mouse {
                    draw_click_ring(frame, *x as i32, *y as i32, event, timestamp_ms, settings);
                }
            }
            OverlayEventKind::MouseDrag { start, end } => {
                if settings.show_mouse {
                    draw_line(
                        frame,
                        start.x as i32,
                        start.y as i32,
                        end.x as i32,
                        end.y as i32,
                        rgba(settings.mouse_primary, 200, settings.opacity),
                        3,
                    );
                }
            }
            OverlayEventKind::MouseWheel { .. } | OverlayEventKind::KeyCombo(_) => {}
        }
    }

    let last_combo = active_events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            OverlayEventKind::KeyCombo(keys) if settings.show_keyboard => Some(keys.as_slice()),
            _ => None,
        });
    if let Some(keys) = last_combo {
        draw_key_combo(frame, keys, settings);
    }

    if settings.show_cursor
        && let Some(point) = timeline.mouse_position_at(timestamp_ms)
    {
        draw_cursor(frame, point.x as i32, point.y as i32, settings);
    }
}

fn ordered_modifiers(keys: &[KeyCode]) -> Vec<KeyCode> {
    [
        KeyCode::Control,
        KeyCode::Shift,
        KeyCode::Alt,
        KeyCode::Meta,
    ]
    .into_iter()
    .filter(|modifier| keys.contains(modifier))
    .collect()
}

fn point_distance(a: Point, b: Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn draw_key_combo(frame: &mut RgbaImage, keys: &[KeyCode], settings: &OverlaySettings) {
    let scale = (settings.label_scale * 2.0).round().clamp(1.0, 6.0) as i32;
    let labels: Vec<String> = keys.iter().map(KeyCode::label).collect();
    let padding_x = 10 * scale;
    let padding_y = 5 * scale;
    let gap = 5 * scale;
    let key_height = 18 * scale;
    let widths: Vec<i32> = labels
        .iter()
        .map(|label| text_width(label, scale, settings.label_font) + padding_x * 2)
        .collect();
    let total_width = widths.iter().sum::<i32>() + gap * (widths.len().saturating_sub(1) as i32);
    let frame_width = frame.width() as i32;
    let frame_height = frame.height() as i32;
    let mut x = ((frame_width - total_width) / 2).max(8);
    let y = match settings.label_position {
        LabelPosition::BottomCenter => frame_height - key_height - 28,
        LabelPosition::TopCenter => 28,
    };

    for (index, label) in labels.iter().enumerate() {
        let width = widths[index];
        fill_rect(
            frame,
            x,
            y,
            width,
            key_height,
            rgba(settings.keyboard_background, 218, settings.opacity),
        );
        draw_rect(
            frame,
            x,
            y,
            width,
            key_height,
            rgba(settings.keyboard_border, 190, settings.opacity),
            2,
        );
        let text_x = x + padding_x;
        let text_y = y + padding_y;
        draw_text(
            frame,
            text_x,
            text_y,
            label,
            scale,
            rgba(settings.keyboard_text, 255, settings.opacity),
            settings.label_font,
        );
        x += width + gap;
    }
}

fn draw_click_ring(
    frame: &mut RgbaImage,
    x: i32,
    y: i32,
    event: &OverlayEvent,
    timestamp_ms: u64,
    settings: &OverlaySettings,
) {
    let elapsed = timestamp_ms.saturating_sub(event.start_ms) as f32;
    let progress = (elapsed / event.duration_ms.max(1) as f32).clamp(0.0, 1.0);
    let base_radius = match event.kind {
        OverlayEventKind::MouseDoubleClick { .. } => 16.0,
        _ => 10.0,
    };
    let radius = base_radius + progress * 28.0;
    let alpha = ((1.0 - progress) * 220.0) as u8;
    draw_circle_outline(
        frame,
        x,
        y,
        radius as i32,
        rgba(settings.mouse_primary, alpha, settings.opacity),
        3,
    );
    draw_circle_outline(
        frame,
        x,
        y,
        (radius * 0.55) as i32,
        rgba(
            settings.mouse_secondary,
            alpha.saturating_sub(50),
            settings.opacity,
        ),
        2,
    );
}

fn text_width(text: &str, scale: i32, font: OverlayLabelFont) -> i32 {
    let advance = match font {
        OverlayLabelFont::Compact => 7,
        OverlayLabelFont::Regular => 8,
        OverlayLabelFont::Bold => 9,
    };
    let count = text.chars().count() as i32;
    if count == 0 {
        0
    } else {
        count * advance * scale
    }
}

fn draw_text(
    frame: &mut RgbaImage,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    color: Rgba<u8>,
    font: OverlayLabelFont,
) {
    use font8x8::UnicodeFonts;

    let mut cursor_x = x;
    let advance = match font {
        OverlayLabelFont::Compact => 7,
        OverlayLabelFont::Regular => 8,
        OverlayLabelFont::Bold => 9,
    } * scale;
    for ch in text.to_ascii_uppercase().chars() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, byte) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if byte & (1 << col) != 0 {
                        fill_rect(
                            frame,
                            cursor_x + col * scale,
                            y + row as i32 * scale,
                            scale,
                            scale,
                            color,
                        );
                        if font == OverlayLabelFont::Bold {
                            fill_rect(
                                frame,
                                cursor_x + col * scale + scale,
                                y + row as i32 * scale,
                                scale,
                                scale,
                                color,
                            );
                        }
                    }
                }
            }
        }
        cursor_x += advance;
    }
}

fn draw_cursor(frame: &mut RgbaImage, x: i32, y: i32, settings: &OverlaySettings) {
    let color = rgba(settings.cursor_color, 235, settings.opacity);
    draw_line(frame, x, y, x + 14, y + 22, color, 3);
    draw_line(frame, x + 14, y + 22, x + 2, y + 18, color, 3);
    draw_line(frame, x + 2, y + 18, x, y, color, 3);
}

fn rgba(color: OverlayColor, a: u8, opacity: f32) -> Rgba<u8> {
    Rgba([
        color.r,
        color.g,
        color.b,
        ((a as f32) * opacity.clamp(0.0, 1.0)) as u8,
    ])
}

fn fill_rect(frame: &mut RgbaImage, x: i32, y: i32, width: i32, height: i32, color: Rgba<u8>) {
    for py in y..y + height {
        for px in x..x + width {
            blend_pixel(frame, px, py, color);
        }
    }
}

fn draw_rect(
    frame: &mut RgbaImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: Rgba<u8>,
    thickness: i32,
) {
    fill_rect(frame, x, y, width, thickness, color);
    fill_rect(frame, x, y + height - thickness, width, thickness, color);
    fill_rect(frame, x, y, thickness, height, color);
    fill_rect(frame, x + width - thickness, y, thickness, height, color);
}

fn draw_circle_outline(
    frame: &mut RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    color: Rgba<u8>,
    thickness: i32,
) {
    let outer = radius * radius;
    let inner = (radius - thickness).max(0).pow(2);
    for y in cy - radius..=cy + radius {
        for x in cx - radius..=cx + radius {
            let dist = (x - cx).pow(2) + (y - cy).pow(2);
            if dist <= outer && dist >= inner {
                blend_pixel(frame, x, y, color);
            }
        }
    }
}

fn draw_line(
    frame: &mut RgbaImage,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    color: Rgba<u8>,
    thickness: i32,
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        fill_rect(
            frame,
            x0 - thickness / 2,
            y0 - thickness / 2,
            thickness,
            thickness,
            color,
        );
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn blend_pixel(frame: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x < 0 || y < 0 || x >= frame.width() as i32 || y >= frame.height() as i32 {
        return;
    }

    let dst = frame.get_pixel_mut(x as u32, y as u32);
    let src_alpha = color[3] as f32 / 255.0;
    let inv_alpha = 1.0 - src_alpha;
    for channel in 0..3 {
        dst[channel] =
            ((color[channel] as f32 * src_alpha) + (dst[channel] as f32 * inv_alpha)) as u8;
    }
    dst[3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_key_combo_overlay_events() {
        let events = vec![
            InputEvent {
                timestamp_ms: 10,
                kind: InputEventKind::KeyDown(KeyCode::Control),
            },
            InputEvent {
                timestamp_ms: 20,
                kind: InputEventKind::KeyDown(KeyCode::Character('S')),
            },
        ];

        let timeline = OverlayTimeline::from_input_events(&events, &OverlaySettings::default());

        assert_eq!(timeline.events.len(), 1);
        assert_eq!(
            timeline.events[0].kind,
            OverlayEventKind::KeyCombo(vec![KeyCode::Control, KeyCode::Character('S')])
        );
    }

    #[test]
    fn builds_click_and_double_click_events() {
        let events = vec![
            InputEvent {
                timestamp_ms: 0,
                kind: InputEventKind::MouseMove { x: 10.0, y: 12.0 },
            },
            InputEvent {
                timestamp_ms: 10,
                kind: InputEventKind::MouseDown(MouseButton::Left),
            },
            InputEvent {
                timestamp_ms: 50,
                kind: InputEventKind::MouseUp(MouseButton::Left),
            },
            InputEvent {
                timestamp_ms: 200,
                kind: InputEventKind::MouseDown(MouseButton::Left),
            },
            InputEvent {
                timestamp_ms: 230,
                kind: InputEventKind::MouseUp(MouseButton::Left),
            },
        ];

        let timeline = OverlayTimeline::from_input_events(&events, &OverlaySettings::default());

        assert_eq!(
            timeline.events[0].kind,
            OverlayEventKind::MouseClick {
                button: MouseButton::Left,
                x: 10.0,
                y: 12.0
            }
        );
        assert!(matches!(
            timeline.events[1].kind,
            OverlayEventKind::MouseDoubleClick { .. }
        ));
    }

    #[test]
    fn renderer_modifies_pixels_for_key_combo() {
        let mut frame = RgbaImage::from_pixel(320, 180, Rgba([0, 0, 0, 255]));
        let timeline = OverlayTimeline {
            events: vec![OverlayEvent {
                start_ms: 0,
                duration_ms: 1_000,
                kind: OverlayEventKind::KeyCombo(vec![KeyCode::Control, KeyCode::Character('S')]),
            }],
            mouse_positions: Vec::new(),
        };

        composite_frame(&mut frame, &timeline, 100, &OverlaySettings::default());

        assert!(
            frame
                .pixels()
                .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0)
        );
    }

    #[test]
    fn renderer_modifies_pixels_for_cursor() {
        let mut frame = RgbaImage::from_pixel(160, 120, Rgba([0, 0, 0, 255]));
        let timeline = OverlayTimeline {
            events: Vec::new(),
            mouse_positions: vec![(0, Point { x: 20.0, y: 20.0 })],
        };
        let settings = OverlaySettings {
            show_cursor: true,
            ..Default::default()
        };

        composite_frame(&mut frame, &timeline, 100, &settings);

        assert!(
            frame
                .pixels()
                .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0)
        );
    }
}
