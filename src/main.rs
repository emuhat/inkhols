// https://dailyverses.net/archive
// https://www.heartlight.org/todaysverse/archive1.html
// https://www.ourdailyverse.com/archive?page=90

// src/main.rs
// Use resvg's re-exported tiny-skia to avoid version conflicts
use chrono::DateTime;
use chrono::Datelike; // For .weekday()
use chrono::Duration;
use chrono::Local;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::Utc;
use flate2::Compression;
use flate2::write::GzEncoder;
use resvg::Tree as ResvgTree; // Also use resvg's re-exported usvg
use resvg::tiny_skia;
use resvg::usvg;
use resvg::usvg::TreeParsing;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use skia_safe::AlphaType;
use skia_safe::Color4f;
use skia_safe::ColorType;
use skia_safe::Data;
use skia_safe::Image;
use skia_safe::ImageInfo;
use skia_safe::Path;
use skia_safe::gradient_shader;
use skia_safe::image::CachingHint;
use skia_safe::{
    Canvas, Color, Font, Paint, PaintStyle, Point, RRect, Rect, Surface, TextBlob, TileMode,
    Typeface,
};
use std::fs;
use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Write;
// use skia_safe::codec::Options;
// use skia_safe::runtime_effect::Options;

/// ---- Data model (from JSON) ----

#[derive(Debug, Deserialize)]
pub struct LastGood {
    pub fetched_at: DateTime<Utc>,
    pub expires: Option<DateTime<Utc>>,
    pub data: Value,
}

#[derive(Debug, Deserialize)]
pub struct StateEnvelope {
    pub status: String,
    pub fetched_at: DateTime<Utc>,
    pub error: Option<String>,
    pub last_good: Option<LastGood>,
}

#[derive(Debug, Deserialize)]
struct Person {
    name: String,
    balance: i64,
    cleaning: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SignificantDate {
    name: String,
    date: String,
    emoji: String,
}

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
    pub temperature: Vec<f32>,
    pub weather_code: Vec<u32>,
    pub precipitation: Vec<f32>,
    pub precipitation_probability: Vec<u32>,
}

// New structs for daily data
#[derive(Debug, Deserialize)]
pub struct DailyWeather {
    pub time: Vec<String>,
    pub weather_code: Vec<u8>,
    #[serde(rename = "temperature_2m_max")]
    pub temperature_max: Vec<f32>,
    #[serde(rename = "temperature_2m_min")]
    pub temperature_min: Vec<f32>,
    #[serde(rename = "precipitation_probability_max")]
    pub precipitation_probability: Vec<f64>,
}

pub struct AllData {
    weather: WeatherResponse,
    weather_age_hours: f64,
    significant_dates: Vec<SignificantDate>,
    people: Vec<Person>,
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

fn days_between(date: NaiveDate) -> i64 {
    let today = Local::now().date_naive();
    (date - today).num_days()
}

const WEEKDAYS2: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
const WEEKDAYS3: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn draw_line(canvas: &mut Canvas, start: Point, end: Point) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgb(200, 200, 200)); // medium gray
    paint.set_anti_alias(true); // Smooth edges
    paint.set_style(PaintStyle::Stroke); // Stroke (not fill)
    paint.set_stroke_width(2.0); // 2px wide
    canvas.draw_line(start, end, &paint);
}

#[allow(dead_code)]
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
    align: f32,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.set_anti_alias(true);

    let xoff = if align > 0.0 {
        let ww = font.measure_str(text, None).0;
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

    pub fn load_bold_font(size: f32) -> Font {
        load_font_from_file("Crimson_Pro/static/CrimsonPro-Bold.ttf", size)
    }

    pub fn new() -> Self {
        // Try to load Roboto but fallback to default if missing.
        let font = Self::load_font(25.0);
        let emoji_font = load_font_from_file("NotoEmoji.ttf", 30.0);

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
    symbol: &str,
    range: (f32, f32),
) {
    // draw_rect_thing(canvas, x, y, width, height);

    let dp_width = (width as f32) / (values.len() - 1) as f32;

    let (min, max) = range;

    let vsize = max - min;

    println!("min {} max {} vsize {}", min, max, vsize);

    let graph_base = 10.0;
    let graph_offset = y as f32 + 5.0;
    let graph_height = (height - 10) as f32 - graph_base;
    let mut temp_points: Vec<Point> = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let val = (values[i] - min) / vsize;

        let px = x as f32 + i as f32 * dp_width;
        let py = graph_offset + (1.0 - val) * graph_height as f32;
        temp_points.push(Point::new(px, py));

        // draw_filled_circle(canvas, Point::new(px, py), 5.0);
    }
    //draw_catmull_rom_curve(canvas, &temp_points);
    fill_catmull_rom_area(
        canvas,
        &temp_points,
        graph_offset + graph_height + graph_base,
    );

    for i in 0..values.len() {
        let pct = i as f32 / (values.len() - 1) as f32;
        // let val = (values[i] - min) / vsize;
        let px = x as f32 + i as f32 * dp_width;
        // let py = graph_offset + val * graph_height as f32;
        if i % 2 == 0 {
            draw_text_blob_with_color(
                canvas,
                &mini_font,
                px as i32,
                (graph_offset + graph_height) as i32 + 28,
                &format!("{}{}", values[i].round(), symbol),
                Color::from_rgb(128, 128, 128),
                pct,
            );
        }
    }
}

fn draw_box_with_gradient(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    top_color: Color,
    bottom_color: Color,
) {
    let margin = 0;

    let rect = Rect::from_xywh(
        (x + margin) as f32,
        (y + margin) as f32,
        (width - margin * 2) as f32,
        (height - margin * 2) as f32,
    );
    let rrect = RRect::new_rect_xy(rect, 8.0, 8.0);

    // --- Gradient fill ---
    let start_color = Color4f::from(top_color);
    let end_color = Color4f::from(bottom_color);

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
    stroke_paint.set_color(Color::BLACK);
    stroke_paint.set_anti_alias(true);
    stroke_paint.set_style(PaintStyle::Stroke);
    stroke_paint.set_stroke_width(2.0);

    canvas.draw_rrect(rrect, &stroke_paint);
}

fn draw_temp_gradient(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32) {
    draw_box_with_gradient(
        canvas,
        x,
        y,
        width,
        height,
        Color::from_rgb(140, 140, 140),
        Color::from_rgb(240, 240, 240),
    );
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
        _ => "partly-cloudy_01.svg", // Default / unknown codes
    }
}

fn code_to_svg(code: u8, dim: u32) -> Result<LoadedSvg, Box<dyn std::error::Error>> {
    let icon_file = format!("weather-icons/{}", wmo_code_to_icon(code));
    svg_from_file(icon_file.as_str(), dim, dim, 1.0)
}

fn get_temp_range(values: &[f32]) -> (f32, f32) {
    let min_range = 30.0;

    let mut min: f32 = 999999.0;
    let mut max: f32 = -999999.0;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }

    let diff = max - min;
    // println!("DIFF {} of {}", diff, min_range);

    if diff < min_range {
        let missing = min_range - diff;
        let missing_half = missing * 0.5;

        min -= missing_half;
        max += missing_half;
    }

    (min, max)
}

fn draw_weather_wrapped(
    canvas: &mut Canvas,
    font_boss: &FontBoss,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    weather: &WeatherResponse,
    weather_age_hours: f64,
) {
    let too_old = weather_age_hours > 1.0;

    let success = !too_old && draw_weather(canvas, font_boss, x, y, width, height, weather);

    if too_old || !success {
        draw_text_blob(canvas, &font_boss.emoji_font, x + 20, y + 40, "😞");

        draw_text_blob(
            canvas,
            &font_boss.main_font,
            x + 70,
            y + 40,
            "Problem getting weather",
        );
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
) -> bool {
    println!(" code {}", weather.current.weather_code);

    let svg = code_to_svg(weather.current.weather_code, 75);

    canvas.draw_image(
        &svg.unwrap().image,
        (x as f32 + 15.0, y as f32 + 10.0),
        None,
    );

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
        x + width + 5,
        y + now_offset + 5,
        &format!(
            "Feels like {}°",
            weather.current.apparent_temperature.round()
        ),
        Color::BLACK,
        1.0,
    );

    draw_text_blob_with_color(
        canvas,
        &font_boss.main_font,
        x + width + 5,
        y + now_offset + 35,
        &format!("Humidity {}%", weather.current.relative_humidity),
        Color::BLACK,
        1.0,
    );

    let today_offset = 110;
    let hourly_height = 80;

    // 1) Get current time (timezone-aware)
    let now_local: DateTime<Local> = Local::now();
    let now_utc: DateTime<Utc> = Utc::now();
    let naive_local: NaiveDateTime = now_local.naive_local();

    println!("local now = {}", now_local);
    println!("utc   now = {}", now_utc);

    println!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~");

    let mut opt_hourly_start_index: Option<usize> = None;
    for i in 0..weather.hourly.time.len() {
        println!("dsfdsf  {}", weather.hourly.time[i]);

        // Parse as a naive datetime (no timezone)
        let dt = NaiveDateTime::parse_from_str(&weather.hourly.time[i], "%Y-%m-%dT%H:%M")
            .expect("Failed to parse datetime");

        if naive_local < dt {
            break;
        }

        opt_hourly_start_index = Some(i);
    }

    if opt_hourly_start_index.is_none() {
        return false;
    }

    let hourly_start_index = opt_hourly_start_index.unwrap();

    let n_forecast_hours = 23;
    let hourly_stop_index = (hourly_start_index + n_forecast_hours).min(weather.hourly.time.len());

    let hourly_x_start = x + 50;
    let hourly_width = width - 47;
    let num_hours = hourly_stop_index - hourly_start_index;

    println!("the thing starts at {}", hourly_start_index);
    println!("the thing stops at {}", hourly_stop_index);
    println!("num_hours {}", num_hours);

    let hourly_slot_width = hourly_width as f32 / (num_hours - 1) as f32;

    println!("hourly_x_start {}", hourly_x_start);
    println!("hourly_slot_width {}", hourly_slot_width);

    let mut precip_points: Vec<f32> = Vec::new();
    let mut temp_points: Vec<f32> = Vec::new();

    for i in hourly_start_index..hourly_stop_index {
        temp_points.push(weather.hourly.temperature[i]);
        precip_points.push(weather.hourly.precipitation_probability[i] as f32);

        let index = i - hourly_start_index;
        let pct = index as f32 / (num_hours - 1) as f32;
        println!(" >> {} -- {}", weather.hourly.time[i], pct);

        // Parse as a naive datetime (no timezone)
        let dt = NaiveDateTime::parse_from_str(&weather.hourly.time[i], "%Y-%m-%dT%H:%M")
            .expect("Failed to parse datetime");

        // Format as 12-hour with AM/PM
        let formatted = dt.format("%-I %p").to_string(); // %-I = hour without leading zero
        // let formatted = dt.format("%-I").to_string();

        println!("{formatted}"); // "12 AM"

        if (index + 3) % 4 == 0 {
            draw_text_blob_with_color(
                canvas,
                &mini_font,
                (hourly_x_start as f32 + index as f32 * hourly_slot_width) as i32,
                y + today_offset,
                &formatted,
                Color::BLACK,
                0.5,
            );
        }
    }

    // temperature curve for today
    let range = get_temp_range(&temp_points);
    draw_text_blob(
        canvas,
        &font_boss.emoji_font,
        x + 10,
        y + today_offset + 60,
        "🌡️",
    );
    draw_hourly(
        canvas,
        &mini_font,
        hourly_x_start,
        y + today_offset + 10,
        hourly_width,
        hourly_height,
        &temp_points,
        "°",
        range,
    );

    draw_text_blob(
        canvas,
        &font_boss.emoji_font,
        x + 10,
        y + today_offset + hourly_height + 30 + 60,
        "💧",
    );
    draw_hourly(
        canvas,
        &mini_font,
        hourly_x_start,
        y + today_offset + hourly_height + 40,
        hourly_width,
        hourly_height,
        &precip_points,
        "%",
        (0.0, 100.0),
    );

    if let Some(daily) = &weather.daily {
        let day_width = 102.0;

        let num_daily_pts = daily.time.len().min(7);

        let mut max_temp: f32 = -99999999.0;
        let mut min_temp: f32 = 99999999.0;
        for i in 0..num_daily_pts {
            max_temp = max_temp.max(daily.temperature_max[i]);
            min_temp = min_temp.min(daily.temperature_max[i]);
            max_temp = max_temp.max(daily.temperature_min[i]);
            min_temp = min_temp.min(daily.temperature_min[i]);
        }
        let temp_range = max_temp - min_temp;

        let max_daily_vpixels_allowed = 105;
        let pixels_per_degree = max_daily_vpixels_allowed as f32 / temp_range;

        println!("min max  {} {}", max_temp, min_temp);
        println!("max degree range {}", temp_range);
        println!("max pixels {}", max_daily_vpixels_allowed);
        println!("pixels per degree {}", pixels_per_degree);

        for i in 0..num_daily_pts {
            // Parse the string into a NaiveDate
            let date = NaiveDate::parse_from_str(&daily.time[i], "%Y-%m-%d").expect("Invalid date");

            // Get the weekday (0 = Monday, 6 = Sunday if you want ISO, or 0 = Sunday with .num_days_from_sunday())
            let weekday = date.weekday().num_days_from_sunday();

            let px = x as f32 + 62.0 + i as f32 * day_width;
            let x = px as i32 - 10;
            let y = y as i32 + 350;

            let qdiff = max_temp - daily.temperature_max[i] as f32;

            let vpushdown = qdiff * pixels_per_degree;

            println!(
                "   {}: {}-{}, {} -> {}",
                i, daily.temperature_min[i], daily.temperature_max[i], qdiff, vpushdown
            );

            let this_grad_off = vpushdown as i32;
            let this_daily_height =
                (pixels_per_degree * (daily.temperature_max[i] - daily.temperature_min[i])) as i32;

            draw_text_blob_with_color(
                canvas,
                &mini_font,
                x,
                y + this_grad_off + 2,
                &format!("{}°", daily.temperature_max[i].round()),
                Color::BLACK,
                0.5,
            );

            draw_temp_gradient(canvas, x - 7, y + this_grad_off + 10, 14, this_daily_height);

            draw_text_blob_with_color(
                canvas,
                &mini_font,
                x,
                y + this_grad_off + this_daily_height + 28,
                &format!("{}°", daily.temperature_min[i].round()),
                Color::BLACK,
                0.5,
            );

            let precip_text = format!("{}%", daily.precipitation_probability[i].round());

            let svg_width = 25;
            let label_margin = 7.0;
            let precip_height = max_daily_vpixels_allowed + 30;
            let day_label_width =
                svg_width as f32 + mini_font.measure_str(&precip_text, None).0 + 5.0;
            let half_width = day_label_width * 0.5;
            let label_start = x as f32 - half_width;

            let svg = code_to_svg(daily.weather_code[i], svg_width);
            canvas.draw_image(
                &svg.unwrap().image,
                (label_start, (y + precip_height) as f32 + 20.0),
                None,
            );

            draw_text_blob_with_color(
                canvas,
                &mini_font,
                (label_start + label_margin) as i32 + svg_width as i32,
                y + precip_height + 39,
                &precip_text,
                Color::BLACK,
                0.0,
            );

            draw_text_blob_with_color(
                canvas,
                &font_boss.main_font,
                x,
                y + precip_height + 70,
                WEEKDAYS3[weekday as usize],
                Color::BLACK,
                0.5,
            );
        }
    }

    true
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

fn measure_and_draw(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    tokens: &Vec<&str>,
    attr: &str,
    fsize: f32,
    draw: bool,
    ypad: i32,
) -> f32 {
    let font = FontBoss::load_font(fsize);
    let spacew = font.measure_str(" ", None).0;
    let padding = 25;

    let raw_lh = line_height(&font) * 0.6;
    let lh = raw_lh * 1.9;
    let mut xp = 0.0;
    let mut yp = raw_lh;

    let target_width = (width - padding * 3) as f32;
    let target_height = (height - padding * 3) as f32;

    if false && draw {
        draw_filled_circle(
            canvas,
            Point::new((x + padding) as f32, (y + padding) as f32),
            3.0,
        );
        draw_filled_circle(
            canvas,
            Point::new((x + padding) as f32 + target_width, (y + padding) as f32),
            3.0,
        );
        draw_filled_circle(
            canvas,
            Point::new(
                (x + padding) as f32 + target_width,
                (y + padding) as f32 + target_height,
            ),
            3.0,
        );
    }

    for token in tokens {
        let ww = font.measure_str(token, None).0;
        // println!("{} - {}", token, ww);

        if xp + ww > target_width {
            xp = 0.0;
            yp += lh;
        }

        if draw {
            draw_text_blob(
                canvas,
                &font,
                xp as i32 + x + padding,
                yp as i32 + y + padding + ypad,
                token,
            );
        }

        xp += ww;
        xp += spacew;
    }

    yp += lh;
    xp = 0.0;

    if draw {
        draw_text_blob_with_color(
            canvas,
            &font,
            xp as i32 + x + padding + target_width as i32,
            yp as i32 + y + padding + ypad,
            attr,
            Color::BLACK,
            1.0,
        );
    }

    // draw at      (y + padding) + yp
    // limit at     (y + padding) as f32 + target_height)

    let leftover = target_height - yp;

    // println!("yp {}  th {}", yp, target_height);

    leftover
}

fn draw_verse(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32) {
    let margin = 15;

    draw_box_with_gradient(
        canvas,
        x + margin,
        y + margin,
        width - margin,
        height - margin * 2,
        Color::from_rgb(220, 220, 220),
        Color::from_rgb(240, 240, 240),
    );

    let attr = "1 Timothy 6:9";
    let verse = "See what great love the Father has lavished on us, that we should be called children of God! And that is what we are! The reason the world does not know us is that it did not know him.";
    // let verse = "Be strong and take heart, all you who hope in the LORD.";
    let tokens: Vec<&str> = verse.split_whitespace().collect();

    for i in (10..=50).rev() {
        let fsize = i as f32 * 0.5;

        let yleftover = measure_and_draw(
            canvas,
            x + margin,
            y + margin,
            width,
            height,
            &tokens,
            &attr,
            fsize,
            false,
            0,
        );

        let fits = yleftover >= 0.0;

        println!("{fsize} -> {fits}");

        if fits {
            // Now we can draw!
            let ypad = (yleftover * 0.5).round();
            let _ = measure_and_draw(
                canvas,
                x + margin,
                y + margin,
                width,
                height,
                &tokens,
                &attr,
                fsize,
                true,
                ypad as i32,
            );

            break;
        }
    }
}

fn format_cents_commas(cents: i64) -> String {
    let dollars = cents / 100;
    let remainder = cents % 100;

    let s = dollars.to_string();
    let mut out = String::new();

    // Insert commas from the right
    let mut count = 0;
    for ch in s.chars().rev() {
        if count == 3 {
            out.push(',');
            count = 0;
        }
        out.push(ch);
        count += 1;
    }

    let dollar_str: String = out.chars().rev().collect();
    format!("${}.{:02}", dollar_str, remainder)
}

fn draw_people(
    canvas: &mut Canvas,
    font_boss: &FontBoss,
    x: i32,
    y: i32,
    width: i32,
    _height: i32,
    data: &AllData,
) {
    let mini_font = FontBoss::load_font(20.0);
    for j in 0..5 {
        draw_text_blob_with_color(
            canvas,
            &mini_font,
            x + width - 205 + 19 + j as i32 * 40,
            y + 10,
            WEEKDAYS2[j],
            Color::from_rgb(128, 128, 128),
            0.5,
        );
    }

    for i in 0..data.people.len() {
        let yoff = y + i as i32 * 50 + 50;
        let person = &data.people[i];

        draw_text_blob(canvas, &font_boss.main_font, x, yoff, &person.name);

        for j in 0..person.cleaning.len() {
            draw_text_blob(
                canvas,
                &font_boss.emoji_font,
                x + width - 205 + j as i32 * 40,
                yoff,
                &person.cleaning[j],
            );
        }

        draw_text_blob_with_color(
            canvas,
            &font_boss.main_font,
            x + width - 240,
            yoff,
            &format_cents_commas(person.balance),
            Color::BLACK,
            1.0,
        );
    }
    // let items = vec![("Edward", 132345), ("Theo", 1354), ("Peter", 1339)];

    // for i in 0..items.len() {
    //     let yoff = y + i as i32 * 50;

    //     // draw_rect_thing(canvas, x, y, width, height);
    //     draw_text_blob(canvas, &font_boss.main_font, x, yoff, items[i].0);
    //     draw_text_blob_with_color(
    //         canvas,
    //         &font_boss.main_font,
    //         x + width - 240,
    //         yoff,
    //         &format_cents_commas(items[i].1),
    //         Color::BLACK,
    //         1.0,
    //     );

    //     draw_text_blob(canvas, &font_boss.emoji_font, x + width - 210, yoff, "❓🤩😀😐❌➖");
    // }
}

fn draw_date(canvas: &mut Canvas, _font_boss: &FontBoss, x: i32, y: i32, width: i32, _height: i32) {
    let font = FontBoss::load_font(35.0);
    let bold_font = FontBoss::load_bold_font(35.0);

    let wday_text = "Saturday";
    let date_text = "November 29";
    let year_text = "2025";

    let wday = font.measure_str(&wday_text, None).0;
    let date = bold_font.measure_str(&date_text, None).0;
    let year = font.measure_str(&year_text, None).0;
    let space = font.measure_str(" ", None).0 * 1.5;

    let lh = line_height(&font);

    draw_text_blob_with_color(
        canvas,
        &font,
        x + width - 10 - (wday + date + year + space * 3.0) as i32,
        (y as f32 + lh) as i32 - 2,
        &wday_text,
        Color::BLACK,
        0.0,
    );

    draw_text_blob_with_color(
        canvas,
        &bold_font,
        x + width - 10 - (date + year + space * 2.0) as i32,
        (y as f32 + lh) as i32 - 2,
        &date_text,
        Color::BLACK,
        0.0,
    );

    draw_text_blob_with_color(
        canvas,
        &font,
        x + width - 10 - (year + space * 1.0) as i32,
        (y as f32 + lh) as i32 - 2,
        &year_text,
        Color::BLACK,
        0.0,
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
            draw_date(canvas, font_boss, x, y, width, height);

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
            // draw_rect_thing(canvas, x, y, width, height);
            // draw_text_blob(canvas, &font_boss.main_font, x, y, "Todo list");
        }
        LayoutNode::Weather(_) => {
            draw_weather_wrapped(
                canvas,
                &font_boss,
                x,
                y,
                width,
                height,
                &data.weather,
                data.weather_age_hours,
            );
        }
        LayoutNode::HLine(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            let hbuf = 0.0;
            let loc = (y as f32 + (y + height) as f32) * 0.5;
            let start = Point::new(x as f32 + hbuf - 19.0, loc); // Start coordinates
            let end = Point::new((x + width) as f32 - hbuf, loc); // End coordinates
            draw_line(canvas, start, end);
        }
        LayoutNode::VLine(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            let vbuf = 0.0;
            let loc = (x as f32 + (x + width) as f32) * 0.5;
            let start = Point::new(loc, y as f32 + vbuf); // Start coordinates
            let end = Point::new(loc, (y + height) as f32 - vbuf); // End coordinates
            draw_line(canvas, start, end);
        }
        LayoutNode::Allowance(_) => {
            draw_people(canvas, font_boss, x, y, width, height, data);
        }
        LayoutNode::Countdown(_) => {
            let sig_dates = &data.significant_dates;

            for i in 0..sig_dates.len() {
                let yoff = y + i as i32 * 45 + 20;

                let target = NaiveDate::parse_from_str(&sig_dates[i].date, "%Y-%m-%d").unwrap();
                let diff = days_between(target);

                // draw_rect_thing(canvas, x, y, width, height);
                draw_text_blob(
                    canvas,
                    &font_boss.emoji_font,
                    x,
                    yoff - 2,
                    &sig_dates[i].emoji,
                );
                draw_text_blob(
                    canvas,
                    &font_boss.main_font,
                    x + 45,
                    yoff,
                    &sig_dates[i].name,
                );
                draw_text_blob_with_color(
                    canvas,
                    &font_boss.main_font,
                    x + width - 25,
                    yoff,
                    &format!("{}", diff),
                    Color::BLACK,
                    1.0,
                );
            }
        }
        LayoutNode::Battery(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            // draw_text_blob(canvas, &font_boss.main_font, x, y, "Battery");
        }
        LayoutNode::Verse(_) => {
            draw_verse(canvas, x, y, width, height);
        }
    }
}

/// Read the inner payload of an envelope file and return (payload, hours_old)
pub fn read_envelope<T: DeserializeOwned>(path: &str) -> io::Result<(T, f64)> {
    let content = fs::read_to_string(path)?;
    let envelope: StateEnvelope =
        serde_json::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let last_good = envelope
        .last_good
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No last_good data in envelope"))?;

    let payload: T = serde_json::from_value(last_good.data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let now = Utc::now();
    let age = now.signed_duration_since(last_good.fetched_at);
    let hours_old = age.num_seconds() as f64 / 3600.0;

    Ok((payload, hours_old))
}

/// ---- Main: read layout.json -> render -> save PNG ----

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (weather, weather_age_hours) = read_envelope::<WeatherResponse>("weather.json")?;
    println!("Data is {:.1} hours old", weather_age_hours);

    let data = fs::read_to_string("dates.json")?;
    let significant_dates: Vec<SignificantDate> = serde_json::from_str(&data)?;

    let data = fs::read_to_string("people.json")?;
    let people: Vec<Person> = serde_json::from_str(&data)?;

    for holiday in &significant_dates {
        println!("{} {} on {}", holiday.emoji, holiday.name, holiday.date);
    }

    let data = AllData {
        weather: weather,
        weather_age_hours: weather_age_hours,
        significant_dates: significant_dates,
        people: people,
    };

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

    // Extract red channel
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut red_channel = Vec::with_capacity(width * height);

    // Read pixels from the image
    let mut pixels = vec![0u8; width * height * 4]; // RGBA format
    if image.read_pixels(
        &skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        ),
        &mut pixels,
        width * 4,
        (0, 0),
        CachingHint::Allow,
    ) {
        // Extract red channel (every 4th byte starting from index 0)
        for i in (0..pixels.len()).step_by(4) {
            red_channel.push(pixels[i]);
        }
    } else {
        return Err("Failed to read pixels".into());
    }

    // Write red channel to gzipped file
    let file = File::create("red_channel.bin.gz")?;
    let buf_writer = BufWriter::new(file);

    // Use GzEncoder with default compression
    let mut encoder = GzEncoder::new(buf_writer, Compression::default());
    encoder.write_all(&red_channel)?;
    encoder.finish()?; // finish() writes the gzip footer and returns the inner writer

    // Write red channel to binary file
    // let file = File::create("red_channel.bin")?;
    // let mut writer = BufWriter::new(file);
    // writer.write_all(&red_channel)?;
    // writer.flush()?;

    let data = image
        .encode_to_data(skia_safe::EncodedImageFormat::PNG)
        .ok_or("Failed to encode image")?;
    let file = File::create("output.png")?;
    let mut writer = BufWriter::new(file);
    writer.write(data.as_bytes())?;

    println!("Saved output.png");

    Ok(())
}
