// src/main.rs
// Use resvg's re-exported tiny-skia to avoid version conflicts
use resvg::tiny_skia;
use resvg::usvg;
use resvg::Tree as ResvgTree; // Also use resvg's re-exported usvg
use serde::Deserialize;
use skia_safe::Color4f;
use skia_safe::ImageInfo;
use skia_safe::ColorType;
use skia_safe::AlphaType;
use resvg::usvg::TreeParsing;
use skia_safe::Data;
use skia_safe::Path;
use skia_safe::gradient_shader;
use skia_safe::{
    Canvas, Color, Font, Paint, PaintStyle, Point, RRect, Rect, Surface, TextBlob, TileMode,
    Typeface,
};
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use skia_safe::Image;
// use skia_safe::codec::Options;
// use skia_safe::runtime_effect::Options;

/// ---- Data model (from JSON) ----

#[derive(Debug, Deserialize)]
pub struct WeatherResponse {
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,

    pub current: CurrentWeather,
    pub hourly: HourlyWeather,
    pub daily: Option<DailyWeather>, // new
}

#[derive(Debug, Deserialize)]
pub struct CurrentWeather {
    pub time: String,
    pub interval: u32,
    pub apparent_temperature: f64,
    #[serde(rename = "temperature_2m")]
    pub temperature: f64,
    pub weather_code: u8,
    #[serde(rename = "relative_humidity_2m")]
    pub relative_humidity: u32,
}

#[derive(Debug, Deserialize)]
pub struct HourlyWeather {
    pub time: Vec<String>,
    #[serde(rename = "temperature_2m")]
    pub temperature: Vec<f64>,
    pub weather_code: Vec<u32>,
    pub precipitation: Vec<f64>,
    pub precipitation_probability: Vec<u32>,
}

// New structs for daily data
#[derive(Debug, Deserialize)]
pub struct DailyWeather {
    pub time: Vec<String>,
    pub weather_code: Vec<u8>,
    #[serde(rename = "temperature_2m_max")]
    pub temperature_max: Vec<f64>,
    #[serde(rename = "temperature_2m_min")]
    pub temperature_min: Vec<f64>,
}

pub struct AllData {
    weather: WeatherResponse,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum LayoutNode {
    #[serde(rename = "container")]
    Container(ContainerNode),

    #[serde(rename = "date")]
    Date(SizedNode),

    #[serde(rename = "todo")]
    Todo(SizedNode),

    #[serde(rename = "hline")]
    HLine(SizedNode),

    #[serde(rename = "vline")]
    VLine(SizedNode),

    #[serde(rename = "weather")]
    Weather(SizedNode),

    #[serde(rename = "allowance")]
    Allowance(SizedNode),

    #[serde(rename = "countdown")]
    Countdown(SizedNode),

    #[serde(rename = "battery")]
    Battery(SizedNode),

    #[serde(rename = "verse")]
    Verse(SizedNode),
}

/// Nodes that *must* have a size.
#[derive(Debug, Deserialize)]
pub struct SizedNode {
    pub size: Size,
}

/// Container nodes do have a size and children.
#[derive(Debug, Deserialize)]
pub struct ContainerNode {
    pub size: Size,
    pub split: SplitDirection,
    pub entries: Vec<LayoutNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Strongly typed size.
///
/// Input formats supported:
///  - "10px" -> Size::Px(10)
///  - "75u"  -> Size::Unit(75.0)
#[derive(Debug)]
pub enum Size {
    Px(u64),
    Unit(f64),
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(px) = s.strip_suffix("px") {
            let value = px.trim().parse::<u64>().map_err(serde::de::Error::custom)?;
            return Ok(Size::Px(value));
        }
        if let Some(u) = s.strip_suffix('u') {
            let value = u.trim().parse::<f64>().map_err(serde::de::Error::custom)?;
            return Ok(Size::Unit(value));
        }
        Err(serde::de::Error::custom(format!(
            "Invalid size '{}', expected like '10px' or '75u'",
            s
        )))
    }
}

/// ---- Trait-based size access to remove boilerplate ----

trait HasSize {
    fn size(&self) -> &Size;
}

impl HasSize for SizedNode {
    fn size(&self) -> &Size {
        &self.size
    }
}

impl HasSize for ContainerNode {
    fn size(&self) -> &Size {
        &self.size
    }
}

impl LayoutNode {
    /// Unified accessor for size — avoids repeated matches throughout the code.
    fn size(&self) -> &Size {
        match self {
            LayoutNode::Container(n) => n.size(),
            LayoutNode::Date(n) => n.size(),
            LayoutNode::Todo(n) => n.size(),
            LayoutNode::HLine(n) => n.size(),
            LayoutNode::VLine(n) => n.size(),
            LayoutNode::Weather(n) => n.size(),
            LayoutNode::Allowance(n) => n.size(),
            LayoutNode::Countdown(n) => n.size(),
            LayoutNode::Battery(n) => n.size(),
            LayoutNode::Verse(n) => n.size(),
        }
    }
}

/// Small helpers to extract numeric values from Size.
fn fixed_from(size: &Size) -> u64 {
    match size {
        Size::Px(v) => *v,
        Size::Unit(_) => 0,
    }
}

fn scaled_from(size: &Size) -> f64 {
    match size {
        Size::Px(_) => 0.0,
        Size::Unit(v) => *v,
    }
}

/// ---- Rendering helpers ----

fn draw_line(canvas: &mut Canvas, start: Point, end: Point) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgb(200, 200, 200)); // medium gray
    paint.set_anti_alias(true); // Smooth edges
    paint.set_style(PaintStyle::Stroke); // Stroke (not fill)
    paint.set_stroke_width(2.0); // 2px wide
    canvas.draw_line(start, end, &paint);
}

fn draw_rect_thing(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32) {
    let margin = 0; //6;
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgb(0, 128, 255));
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width(2.0);

    let rect = Rect::from_xywh(
        (x + margin) as f32,
        (y + margin) as f32,
        (width - margin * 2) as f32,
        (height - margin * 2) as f32,
    );
    let rrect = RRect::new_rect_xy(rect, 8.0, 8.0);
    canvas.draw_rrect(rrect, &paint);
}

fn line_height(font: &Font) -> f32 {
    // returns (size, metrics)
    let (_size, metrics) = font.metrics();

    // ascent is negative, descent is positive
    (metrics.descent - metrics.ascent + metrics.leading).abs()
}

fn draw_text_blob_with_color(
    canvas: &mut Canvas,
    font: &Font,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    align: f32
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.set_anti_alias(true);

    let xoff = if align > 0.0 {
        let ww = font.measure_str(text, Some(&paint)).0;
        -align * ww
    } else {
        0.0
    };

    if let Some(blob) = TextBlob::from_str(text, &font) {
        canvas.draw_text_blob(&blob, (x as f32 + xoff, y as f32), &paint);
    } else {
        // fallback: nothing fancy, shouldn't normally happen
        canvas.draw_str(text, (x as f32 + xoff, y as f32), &font, &paint);
    }
}

fn draw_text_blob(canvas: &mut Canvas, font: &Font, x: i32, y: i32, text: &str) {
    draw_text_blob_with_color(canvas, font, x, y, text, Color::BLACK, 0.0);
}

fn load_font_from_file(path: &str, size: f32) -> Font {
    // Load the font file into memory
    let font_bytes = fs::read(path).expect("Failed to read font file");
    let data = Data::new_copy(&font_bytes);

    // Create the typeface from the in-memory data
    let tf = Typeface::from_data(data, 0).unwrap_or_else(|| Typeface::default());

    Font::new(tf, size)
}

struct FontBoss {
    pub main_font: Font,
    pub emoji_font: Font,
}

impl FontBoss {
    pub fn load_font(size: f32) -> Font {
        load_font_from_file("Crimson_Pro/static/CrimsonPro-Regular.ttf", size)
    }

    pub fn new() -> Self {
        // Try to load Roboto but fallback to default if missing.
        let font = Self::load_font(25.0);
        let emoji_font = load_font_from_file("NotoEmoji.ttf", 25.0);

        FontBoss {
            main_font: font,
            emoji_font: emoji_font,
        }
    }
}

/// ---- Layout engine: container splitting and child dispatch ----

fn handle_container(
    canvas: &mut Canvas,
    font_boss: &FontBoss,
    container: &ContainerNode,
    split: &SplitDirection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    data: &AllData,
) {
    // Determine the dimension (in pixels) along which we split children.
    let split_dim_pix = match split {
        SplitDirection::Horizontal => width,
        SplitDirection::Vertical => height,
    };

    // 1) Sum fixed pixels among children
    let mut fixed_sum: u64 = 0;
    for child in &container.entries {
        fixed_sum += fixed_from(child.size());
    }

    // 2) Compute leftover to be distributed to "Unit" children.
    let leftover = split_dim_pix.saturating_sub(fixed_sum as i32);

    // 3) Sum the scaled units among children
    let mut scale_sum = 0.0f64;
    for child in &container.entries {
        scale_sum += scaled_from(child.size());
    }

    // 4) compute each child's size and start offset
    let mut sizes: Vec<i32> = Vec::with_capacity(container.entries.len());
    let mut starts: Vec<i32> = Vec::with_capacity(container.entries.len());
    let mut cursor = 0i32;

    for child in &container.entries {
        starts.push(cursor);

        let fs = fixed_from(child.size());
        let ss = scaled_from(child.size());

        let child_size = if fs > 0 {
            fs as i32
        } else {
            // if scale_sum is zero (no scalable children) but leftover > 0, give zero
            if scale_sum <= 0.0 {
                0
            } else {
                ((leftover as f64) * (ss / scale_sum)) as i32
            }
        };

        sizes.push(child_size);
        cursor += child_size;
    }

    // 5) Dispatch each child
    for (i, child) in container.entries.iter().enumerate() {
        let sx = starts[i];
        let s = sizes[i];

        match split {
            SplitDirection::Horizontal => {
                handle_child(canvas, font_boss, &child, x + sx, y, s, height, data);
            }
            SplitDirection::Vertical => {
                handle_child(canvas, font_boss, &child, x, y + sx, width, s, data);
            }
        }
    }
}

// Draws a smooth Catmull-Rom spline through the points
// and fills the area under it down to the baseline.
fn fill_catmull_rom_area(canvas: &mut Canvas, points: &[Point], baseline_y: f32) {
    if points.len() < 2 {
        return;
    }

    // --- Create the fill path ---
    let mut fill_path = Path::new();
    fill_path.move_to(Point::new(points[0].x, baseline_y)); // baseline start
    fill_path.line_to(points[0]); // move up to first point

    // Build Catmull-Rom spline
    for i in 0..points.len() - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < points.len() {
            points[i + 2]
        } else {
            points[points.len() - 1]
        };

        let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
        let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);

        fill_path.cubic_to(c1, c2, p2);
    }

    // Line down to baseline at last x
    fill_path.line_to(Point::new(points[points.len() - 1].x, baseline_y));
    // Close path (connects back to baseline start)
    fill_path.close();

    // --- Fill area ---
    let mut paint_fill = Paint::default();
    paint_fill.set_color(Color::from_rgb(220, 220, 220));
    paint_fill.set_anti_alias(true);
    paint_fill.set_style(PaintStyle::Fill);
    canvas.draw_path(&fill_path, &paint_fill);

    // --- Stroke only the curve ---
    let mut curve_path = Path::new();
    curve_path.move_to(points[0]);

    for i in 0..points.len() - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < points.len() {
            points[i + 2]
        } else {
            points[points.len() - 1]
        };

        let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
        let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);

        curve_path.cubic_to(c1, c2, p2);
    }

    let mut paint_curve = Paint::default();
    paint_curve.set_color(Color::BLACK);
    paint_curve.set_style(PaintStyle::Stroke);
    paint_curve.set_stroke_width(2.0);
    paint_curve.set_anti_alias(true);

    canvas.draw_path(&curve_path, &paint_curve);
}

#[allow(dead_code)]
fn draw_filled_circle(canvas: &mut Canvas, center: Point, radius: f32) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgb(200, 50, 50)); // red color
    paint.set_anti_alias(true); // smooth edges
    paint.set_style(PaintStyle::Fill); // fill, not stroke

    canvas.draw_circle(center, radius, &paint);
}

fn draw_hourly(
    canvas: &mut Canvas,
    mini_font: &Font,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    values: &[f32],
) {
    // draw_rect_thing(canvas, x, y, width, height);

    let margin = 15;
    let dp_width = ((width - margin * 2) as f32) / (values.len() - 1) as f32;

    let mut min: f32 = 999999.0;
    let mut max: f32 = -999999.0;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    let vsize = max - min;

    println!("min {} max {} vsize {}", min, max, vsize);

    let graph_base = 10.0;
    let graph_offset = y as f32 + 5.0;
    let graph_height = (height - 10) as f32 - graph_base;
    let mut temp_points: Vec<Point> = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let val = (values[i] - min) / vsize;
        let px = (x + margin) as f32 + i as f32 * dp_width;
        let py = graph_offset + val * graph_height as f32;
        temp_points.push(Point::new(px, py));

        // draw_filled_circle(canvas, Point::new(px, py), 5.0);

        if i % 2 == 0 {
            draw_text_blob_with_color(
                canvas,
                &mini_font,
                px as i32 - 6,
                py as i32 - 8,
                "32",
                Color::from_rgb(128, 128, 128),
                0.0
            );
        }
    }
    //draw_catmull_rom_curve(canvas, &temp_points);
    fill_catmull_rom_area(
        canvas,
        &temp_points,
        graph_offset + graph_height + graph_base,
    );
}

fn draw_temp_gradient(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32) {
    let margin = 0;

    let rect = Rect::from_xywh(
        (x + margin) as f32,
        (y + margin) as f32,
        (width - margin * 2) as f32,
        (height - margin * 2) as f32,
    );
    let rrect = RRect::new_rect_xy(rect, 8.0, 8.0);

    // --- Gradient fill ---
    let start_color = Color4f::from(Color::from_rgb(140, 140, 140));
    let end_color = Color4f::from(Color::from_rgb(240, 240, 240));

    // make a slice explicitly
    let colors_slice: &[Color4f] = &[start_color, end_color];

    let shader = gradient_shader::linear(
        (
            Point::new(rect.left, rect.top),
            Point::new(rect.left, rect.bottom),
        ), // tuple of points
        colors_slice, // explicit slice
        None,
        TileMode::Clamp,
        None,
        None,
    );

    let mut fill_paint = Paint::default();
    fill_paint.set_anti_alias(true);
    fill_paint.set_style(PaintStyle::Fill);
    fill_paint.set_shader(shader);

    canvas.draw_rrect(rrect, &fill_paint);

    // --- Black outline ---
    let mut stroke_paint = Paint::default();
    stroke_paint.set_color(Color::from_argb(80, 0, 0, 0));
    stroke_paint.set_anti_alias(true);
    stroke_paint.set_style(PaintStyle::Stroke);
    stroke_paint.set_stroke_width(2.0);

    canvas.draw_rrect(rrect, &stroke_paint);
}

fn wmo_code_to_icon(code: u8) -> &'static str {
    match code {
        0 => "sunny-29.svg",                  // Clear sky
        1 | 2 | 3 => "partly-cloudy-5.svg",   // Mainly clear, partly cloudy, overcast
        45 | 48 => "fog-85.svg",              // Fog / depositing rime fog
        51 | 53 | 55 => "light-rain-90.svg",  // Drizzle
        56 | 57 => "sleet_03.svg",            // Freezing drizzle
        61 | 63 | 65 => "shower-rain-1.svg",  // Rain showers
        66 | 67 => "sleet_04.svg",            // Freezing rain
        71 | 73 | 75 => "slight-snow_01.svg", // Snow fall
        77 => "slight-snow.svg",              // Snow grains
        80 | 81 | 82 => "shower-rain-1.svg",  // Rain showers
        85 | 86 => "medium-snow_01.svg",      // Snow showers
        95 => "thunderstorm-24.svg",          // Thunderstorm
        96 | 99 => "thunder-47.svg",          // Thunderstorm with hail
        // Some extreme / less common cases
        61..=67 => "shower-rain-1.svg",
        70..=79 => "medium-snow_01.svg",
        _ => "partly-cloudy_01.svg",          // Default / unknown codes
    }
}

fn draw_weather(
    canvas: &mut Canvas,
    font_boss: &FontBoss,
    x: i32,
    y: i32,
    width: i32,
    _height: i32,
    weather: &WeatherResponse,
) {
    println!(" code {}", weather.current.weather_code);

    let icon_file = format!("weather-icons/{}", wmo_code_to_icon(weather.current.weather_code));
    let svg = svg_from_file(icon_file.as_str(), 75, 75, 1.0);

    canvas.draw_image(&svg.unwrap().image, (x as f32 + 15.0, y as f32 + 10.0), None);

    let n_forecast_days = 7;
    let day_width = 100.0;

    let mini_font = FontBoss::load_font(20.0);
    let mega_font = FontBoss::load_font(100.0);
    let now_offset = 30;

    draw_text_blob(
        canvas,
        &mega_font,
        x + 105,
        y + now_offset + 45,
        &format!("{}°", weather.current.temperature.round()),
    );

    draw_text_blob_with_color(
        canvas,
        &font_boss.main_font,
        x + width - 10,
        y + now_offset + 5,
        &format!(
            "Feels like {}°",
            weather.current.apparent_temperature.round()
        ),
        Color::BLACK,
        1.0
    );

    draw_text_blob_with_color(
        canvas,
        &font_boss.main_font,
        x + width - 10,
        y + now_offset + 35,
        &format!("Humidity {}%", weather.current.relative_humidity),
        Color::BLACK,
        1.0
    );

    for i in 0..n_forecast_days {
        draw_text_blob(
            canvas,
            &mini_font,
            (x as f32 + 25.0 + i as f32 * day_width) as i32,
            y + 325,
            "12 PM",
        );
    }

    let today_offset = 110;
    let hourly_height = 80;

    // temperature curve for today
    let mut temp_points: Vec<f32> = Vec::with_capacity(n_forecast_days);
    temp_points.push(33.0);
    temp_points.push(33.0);
    temp_points.push(32.0);
    temp_points.push(31.0);
    temp_points.push(32.0);
    temp_points.push(33.0);
    temp_points.push(30.0);
    temp_points.push(29.0);
    temp_points.push(28.0);
    draw_hourly(
        canvas,
        &mini_font,
        x + 10,
        y + today_offset,
        width - 20,
        hourly_height,
        &temp_points,
    );

    let mut precip_points: Vec<f32> = Vec::new();
    precip_points.push(4.0);
    precip_points.push(4.0);
    precip_points.push(4.0);
    precip_points.push(5.0);
    precip_points.push(7.0);
    precip_points.push(7.0);
    precip_points.push(7.0);
    precip_points.push(7.0);
    precip_points.push(7.0);
    draw_hourly(
        canvas,
        &mini_font,
        x + 10,
        y + today_offset + hourly_height + 30,
        width - 20,
        hourly_height,
        &precip_points,
    );

    // draw_catmull_rom_curve(canvas, &temp_points);

    // precip curve for today
    // let precip_offset = 90;
    // let precip_height = 20.0;
    // let mut precip_points: Vec<Point> = Vec::with_capacity(n_forecast_days);
    // let n_forecast_hours = 24;
    // for i in 0..n_forecast_hours {
    //     let px = x as f32 + 30.0 + i as f32 * 28.0;
    //     let py = y as f32 + today_offset as f32 + ((i as f32 * 50.0).sin() * precip_height + precip_height*0.5 + precip_offset as f32);
    //     precip_points.push(Point::new(px, py));

    //     if i % 2 == 0 {
    //         draw_text_blob(canvas, &mini_font, px as i32, y + precip_offset, "32");
    //     }
    // }
    // draw_catmull_rom_curve(canvas, &precip_points);

    // for i in 0..7 {
    //     draw_text_blob(canvas, &mini_font, x + 30 + i * 95, y + today_offset + 105, "12 PM");
    // }

    let daily_height = 80;
    for i in 0..n_forecast_days {
        let px = x as f32 + 50.0 + i as f32 * day_width;

        let x = px as i32 - 10;
        let y = y as i32 + 350;

        draw_text_blob_with_color(canvas, &mini_font, x, y + 10, "80°", Color::BLACK, 0.5);

        draw_temp_gradient(canvas, x - 7, y + 20, 14, daily_height);

        draw_text_blob_with_color(canvas, &mini_font, x, y + daily_height + 40, "20°", Color::BLACK, 0.5);

        draw_text_blob_with_color(canvas, &mini_font, x, y + daily_height + 80, "83%", Color::BLACK, 0.5);
        draw_text_blob_with_color(
            canvas,
            &font_boss.main_font,
            x,
            y + daily_height + 110,
            "Sat",
            Color::BLACK, 0.5
        );
    }
}

pub struct LoadedSvg {
    image: Image, // Final pre-rendered Skia image
    pub width: f32,
    pub height: f32,
}

pub fn svg_from_file(
    path: &str,
    target_width: u32,
    target_height: u32,
    scalar: f32,
) -> Result<LoadedSvg, Box<dyn std::error::Error>> {

    let svg_data = std::fs::read(path)?;

    let options = usvg::Options::default();
    let usvg_tree = usvg::Tree::from_data(&svg_data, &options)?;
    let resvg_tree = ResvgTree::from_usvg(&usvg_tree);

    // Render to full target size
    let mut pixmap =
        tiny_skia::Pixmap::new(target_width, target_height).ok_or("Failed to create pixmap")?;

    let svg_size = resvg_tree.size;
    let scale_x = target_width as f32 / svg_size.width();
    let scale_y = target_height as f32 / svg_size.height();
    let scale = scale_x.min(scale_y) * scalar; // Maintain aspect ratio

    // Calculate translation to center the scaled image
    let scaled_width = svg_size.width() * scale;
    let scaled_height = svg_size.height() * scale;

    let transform = tiny_skia::Transform::from_translate(0.0, 0.0).post_scale(scale, scale);

    resvg_tree.render(transform, &mut pixmap.as_mut());

    // Convert pixmap to skia_safe::Image
    let image_info = ImageInfo::new(
        (target_width as i32, target_height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );

    let image = Image::from_raster_data(
        &image_info,
        Data::new_copy(pixmap.data()),
        (target_width * 4) as usize,
    )
    .ok_or("Failed to create Skia image")?;

    Ok(LoadedSvg {
        image,
        width: scaled_width as f32,
        height: scaled_height as f32,
    })
}

fn draw_verse(
    canvas: &mut Canvas,
    font_boss: &FontBoss,
    x: i32,
    y: i32,
    _width: i32,
    _height: i32,
) {
    let y_off = 40;
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off,
        "The king’s scribes were summoned at that time, in the third month,",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 1,
        "which is the month of Sivan, on the twenty-third day. And an edict",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 2,
        "was written, according to all that Mordecai commanded concerning",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 3,
        "the Jews, to the satraps and the governors and the officials of the",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 4,
        "provinces from India to Ethiopia, 127 provinces, to each province in its",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 5,
        "own script and to each people in its own language, and also to the Jews",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 6,
        "in their script and their language.",
    );
    draw_text_blob(
        canvas,
        &font_boss.main_font,
        x + 10,
        y + y_off + 30 * 7,
        "Esther 8:9",
    );
}

fn handle_child(
    canvas: &mut Canvas,
    font_boss: &FontBoss,
    node: &LayoutNode,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    data: &AllData,
) {
    match node {
        LayoutNode::Container(container) => {
            handle_container(
                canvas,
                font_boss,
                container,
                &container.split,
                x,
                y,
                width,
                height,
                data,
            );
        }
        LayoutNode::Date(_) => {
            let font = FontBoss::load_font(35.0);

            let lh = line_height(&font);

            draw_text_blob(
                canvas,
                &font,
                x + 10,
                (y as f32 + lh) as i32 - 2,
                "Saturday November 29",
            );
            // draw_rect_thing(canvas, x, y, width, height);

            // date: render a filled rounded rect and big text
            // let mut paint = Paint::default();
            // paint.set_color(Color::from_rgb(240, 240, 240));
            // paint.set_anti_alias(true);
            // paint.set_style(PaintStyle::Fill);

            // let rect = Rect::from_xywh(x as f32 + 4.0, y as f32 + 4.0, width as f32 - 8.0, height as f32 - 8.0);
            // let rrect = RRect::new_rect_xy(rect, 8.0, 8.0);
            // canvas.draw_rrect(rrect, &paint);
        }
        LayoutNode::Todo(_) => {
            draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Todo list");
        }
        LayoutNode::Weather(_) => {
            draw_weather(canvas, &font_boss, x, y, width, height, &data.weather);
        }
        LayoutNode::HLine(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            let hbuf = 50.0;
            let loc = (y as f32 + (y + height) as f32) * 0.5;
            let start = Point::new(x as f32 + hbuf, loc); // Start coordinates
            let end = Point::new((x + width) as f32 - hbuf, loc); // End coordinates
            draw_line(canvas, start, end);
        }
        LayoutNode::VLine(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            let vbuf = 50.0;
            let loc = (x as f32 + (x + width) as f32) * 0.5;
            let start = Point::new(loc, y as f32 + vbuf); // Start coordinates
            let end = Point::new(loc, (y + height) as f32 - vbuf); // End coordinates
            draw_line(canvas, start, end);
        }
        LayoutNode::Allowance(_) => {
            draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Allowance");
        }
        LayoutNode::Countdown(_) => {
            draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.emoji_font, x, y, "🎂");
            draw_text_blob(canvas, &font_boss.main_font, x + 40, y, "Greg");
        }
        LayoutNode::Battery(_) => {
            draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Battery");
        }
        LayoutNode::Verse(_) => {
            draw_verse(canvas, &font_boss, x, y, width, height);
        }
    }
}

/// ---- Main: read layout.json -> render -> save PNG ----

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string("weather.json")?;
    let weather: WeatherResponse = serde_json::from_str(&json)?;

    let data = AllData { weather: weather };

    // println!("{:#?}", weather.current.temperature);

    let contents = fs::read_to_string("layout.json")
        .map_err(|e| format!("Failed to read layout.json: {}", e))?;
    let root: LayoutNode = serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse layout.json: {}", e))?;

    let width = 1200;
    let height = 825;

    let font_boss = FontBoss::new();

    let mut surface =
        Surface::new_raster_n32_premul((width, height)).expect("Failed to create Skia surface");
    let canvas = surface.canvas();

    // white background
    canvas.clear(Color::WHITE);

    if let LayoutNode::Container(ref container) = root {
        handle_container(
            canvas,
            &font_boss,
            container,
            &container.split,
            0,
            0,
            width,
            height,
            &data,
        );
    } else {
        println!("Root of layout.json must be a container node.");
    }

    // Optional: draw a paragraph demo in the top-left (uncomment to see)
    // draw_paragraph_demo(canvas, 60.0, 60.0, 600.0);

    // Save to PNG
    let image = surface.image_snapshot();
    let data = image
        .encode_to_data(skia_safe::EncodedImageFormat::PNG)
        .ok_or("Failed to encode image")?;
    let file = File::create("output.png")?;
    let mut writer = BufWriter::new(file);
    writer.write(data.as_bytes())?;

    println!("Saved output.png");

    Ok(())
}
