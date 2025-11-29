// src/main.rs
use serde::Deserialize;
use skia_safe::{
    Color, Font, Paint, PaintStyle, Rect, RRect, Surface, TextBlob, Typeface,
    Canvas,
};
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use skia_safe::Data;

/// ---- Data model (from JSON) ----

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum LayoutNode {
    #[serde(rename = "container")]
    Container(ContainerNode),

    #[serde(rename = "date")]
    Date(SizedNode),

    #[serde(rename = "todo")]
    Todo(SizedNode),

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

fn draw_rect_thing(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32) {
    let margin = 6;
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

fn draw_text_blob(canvas: &mut Canvas, font: &Font, x: i32, y: i32, text: &str, size: f32) {
    // Try to load Roboto but fallback to default if missing.
    // let tf = Typeface::new("Roboto", FontStyle::normal()).unwrap_or(Typeface::default());
    // let font = Font::new(tf, size);

    let mut paint = Paint::default();
    paint.set_color(Color::BLACK);
    paint.set_anti_alias(true);

    if let Some(blob) = TextBlob::from_str(text, &font) {
        canvas.draw_text_blob(&blob, (x as f32 + 8.0, y as f32 + size + 4.0), &paint);
    } else {
        // fallback: nothing fancy, shouldn't normally happen
        canvas.draw_str(text, (x as f32 + 8.0, y as f32 + size + 4.0), &font, &paint);
    }
}

fn load_font_from_file(path: &str, size: f32) -> Font {
    // Load the font file into memory
    let font_bytes = fs::read(path).expect("Failed to read font file");
    let data = Data::new_copy(&font_bytes);

    // Create the typeface from the in-memory data
    let tf = Typeface::from_data(data, 0).unwrap_or_else(|| Typeface::default());

    Font::new(tf, size)
}

struct FontBoss
{
    pub main_font: Font,
    pub emoji_font: Font,
}

impl FontBoss
{
    pub fn load_font(size: f32) -> Font
    {
        load_font_from_file("Crimson_Pro/static/CrimsonPro-Regular.ttf", size)
    }

    pub fn new() -> Self
    {
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
                handle_child(canvas, font_boss, &child, x + sx, y, s, height);
            }
            SplitDirection::Vertical => {
                handle_child(canvas, font_boss, &child, x, y + sx, width, s);
            }
        }
    }
}

fn handle_child(canvas: &mut Canvas, font_boss: &FontBoss, node: &LayoutNode, x: i32, y: i32, width: i32, height: i32) {
    match node {
        LayoutNode::Container(container) => {
            handle_container(canvas, font_boss, container, &container.split, x, y, width, height);
        }
        LayoutNode::Date(_) => {
            // date: render a filled rounded rect and big text
            // let mut paint = Paint::default();
            // paint.set_color(Color::from_rgb(240, 240, 240));
            // paint.set_anti_alias(true);
            // paint.set_style(PaintStyle::Fill);

            // let rect = Rect::from_xywh(x as f32 + 4.0, y as f32 + 4.0, width as f32 - 8.0, height as f32 - 8.0);
            // let rrect = RRect::new_rect_xy(rect, 8.0, 8.0);
            // canvas.draw_rrect(rrect, &paint);

            let font = FontBoss::load_font(35.0);
            draw_text_blob(canvas, &font, x, y + 4, "Saturday November 29", 20.0);
        }
        LayoutNode::Todo(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Todo list", 14.0);
        }
        LayoutNode::Weather(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Weather °F", 14.0);
        }
        LayoutNode::Allowance(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Allowance", 14.0);
        }
        LayoutNode::Countdown(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.emoji_font, x, y, "🎂", 14.0);
            draw_text_blob(canvas, &font_boss.main_font, x + 40, y, "Greg", 14.0);
        }
        LayoutNode::Battery(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Battery", 14.0);
        }
        LayoutNode::Verse(_) => {
            // draw_rect_thing(canvas, x, y, width, height);
            draw_text_blob(canvas, &font_boss.main_font, x, y, "Verse", 14.0);
        }
    }
}

/// ---- Main: read layout.json -> render -> save PNG ----

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let contents = fs::read_to_string("layout.json")
        .map_err(|e| format!("Failed to read layout.json: {}", e))?;
    let root: LayoutNode = serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse layout.json: {}", e))?;

    let width = 1200;
    let height = 825;

    let font_boss = FontBoss::new();

    let mut surface = Surface::new_raster_n32_premul((width, height))
        .expect("Failed to create Skia surface");
    let canvas = surface.canvas();

    // white background
    canvas.clear(Color::WHITE);

    if let LayoutNode::Container(ref container) = root {
        handle_container(canvas, &font_boss, container, &container.split, 0, 0, width, height);
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
