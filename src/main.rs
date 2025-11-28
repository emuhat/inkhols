// Cargo.toml
// [dependencies]
// skia-safe = { version = "0.60.0", features = ["textlayout"] }
// font-kit = "0.14.0"

use std::io::Write;
use std::fs;
use skia_safe::Canvas;

use skia_safe::{
    textlayout::{FontCollection, ParagraphBuilder, ParagraphStyle, TextStyle, Paragraph},
    Color, Paint, Rect, RRect, Surface, Typeface, FontMgr,
};
use std::fs::File;
use std::io::BufWriter;
use skia_safe::PaintStyle;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum LayoutNode {
    #[serde(rename = "container")]
    Container(ContainerNode),

    #[serde(rename = "header")]
    Header(SizedNode),

    #[serde(rename = "todo")]
    Todo(SizedNode),
}

/// Nodes that *must* have a size.
#[derive(Debug, Deserialize)]
pub struct SizedNode {
    pub size: Size,
}

/// Container nodes do not have a size.
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
/// - "10px" → Size::Px(10)
/// - "75%" → Size::Unit(75)
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

        if let Some(px_str) = s.strip_suffix("px") {
            let value = px_str.trim().parse::<u64>().map_err(serde::de::Error::custom)?;
            return Ok(Size::Px(value));
        }

        if let Some(unit_str) = s.strip_suffix('u') {
            let value = unit_str.trim().parse::<f64>().map_err(serde::de::Error::custom)?;
            return Ok(Size::Unit(value));
        }

        Err(serde::de::Error::custom(format!(
            "Invalid size format '{}', expected \\d+px or \\d+u",
            s
        )))
    }
}

fn get_scaled_size_from_size(size: &Size) -> f64
{
    match &size {
        Size::Px(_) => {
            0.0
        }

        Size::Unit(val) => {
            *val
        }
    }
}

fn get_fixed_size_from_size(size: &Size) -> u64
{
    match &size {
        Size::Px(val) => {
            *val
        }

        Size::Unit(_) => {
            0
        }
    }
}

fn get_fixed_val(bar: &LayoutNode) -> u64 {
    match &bar
    {
        LayoutNode::Container(container) => {
            get_fixed_size_from_size(&container.size)
        }

        LayoutNode::Header(header) => {
            get_fixed_size_from_size(&header.size)
        }

        LayoutNode::Todo(todo) => {
            get_fixed_size_from_size(&todo.size)
        }
    }
}

fn get_scaled_val(bar: &LayoutNode) -> f64 {
    match &bar
    {
        LayoutNode::Container(container) => {
            get_scaled_size_from_size(&container.size)
        }

        LayoutNode::Header(header) => {
            get_scaled_size_from_size(&header.size)
        }

        LayoutNode::Todo(todo) => {
            get_scaled_size_from_size(&todo.size)
        }
    }
}

fn handle_container(canvas: &mut Canvas, container: &ContainerNode, split:&SplitDirection, x:i32, y:i32, width: i32, height: i32) {
    println!("Container: {}", container.entries.len());
    println!("container size {:?}", container.size);

    let split_dim_pix = match split {
        SplitDirection::Horizontal => width,
        SplitDirection::Vertical => height,
    };

    println!("split_dim_pix {}", split_dim_pix);

    let mut fsum : u64 = 0;
    for child in &container.entries {
        fsum += get_fixed_val(child);
    }

    println!("fsum {}", fsum);

    let leftover = split_dim_pix - fsum as i32;

    println!("leftover {}", leftover);

    let mut ssum = 0.0;
    for child in &container.entries {
        ssum += get_scaled_val(child);
    }

    println!("ssum {}", ssum);

    let mut sizen: Vec<i32> = Vec::new();
    let mut starts: Vec<i32> = Vec::new();

    let mut adv:i32 = 0;

    for child in &container.entries {

        starts.push(adv);

        let fs = get_fixed_val(child);
        let ss = get_scaled_val(child);

        // sizen[index] = adv;

        let sz = if fs > 0 {
            println!("fixed {}", fs);

            (fs as i32).try_into().unwrap()
        } else {
            let scaled = leftover as f64 * (ss / ssum);
            println!("scaled {}, {}", ss, scaled);

            (scaled as i32).try_into().unwrap()
        };

        sizen.push(sz);
        adv += sz;
    }

    for i in 0 .. sizen.len() {
        println!("heyo {} {} {}", i, starts[i], sizen[i]);

        match split {
            SplitDirection::Horizontal => {
                handle_child(canvas, &container.entries[i], x + starts[i], y, sizen[i], height);
            }
            SplitDirection::Vertical => {
                handle_child(canvas, &container.entries[i], x, y + starts[i], width, sizen[i]);
            }
        }


    }


}

fn draw_rect_thing(canvas: &mut Canvas, x:i32, y:i32, width: i32, height: i32)
{
    let margin = 4;

    // --- Rounded rectangle ---
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgb(0, 128, 255));
    paint.set_anti_alias(true);

    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width(2.0);

    let rect = Rect::from_xywh((x + margin) as f32, (y + margin) as f32, (width - margin*2) as f32, (height - margin*2) as f32);
    let rrect = RRect::new_rect_xy(rect, 5.0, 5.0);
    canvas.draw_rrect(rrect, &paint);
}

fn handle_child(canvas: &mut Canvas, bar: &LayoutNode, x:i32, y:i32, width: i32, height: i32) {
    println!("handle_child!! {} {} {} {}", x, y, width, height);

    //




    match &bar
    {
        LayoutNode::Container(container) => {

            handle_container(canvas, container, &container.split, x, y, width, height);
        }

        LayoutNode::Header(header) => {

            println!(".... header? {:?}", header.size);

            draw_rect_thing(canvas, x, y, width, height);
        }


        LayoutNode::Todo(todo) => {

            println!(".... todo? {:?}", todo.size);

            draw_rect_thing(canvas, x, y, width, height);
        }
    };
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let contents = fs::read_to_string("layout.json")?;
    let root: LayoutNode = serde_json::from_str(&contents)?;

    let width = 1200;
    let height = 825;

    // println!("{:#?}", root);


    // split_container(&root, width, height);



    let mut surface = Surface::new_raster_n32_premul((width, height))
        .expect("Failed to create surface");
    let canvas = surface.canvas();

    // Background
    canvas.clear(Color::WHITE);

    if let LayoutNode::Container(ref container) = root {
        handle_container(canvas, container, &container.split, 0, 0, width, height);
    }

    // --- Style 1 (Roboto) ---
    if false {
        // --- Font collection ---
        let mut font_collection = FontCollection::new();
        font_collection.set_default_font_manager(FontMgr::default(), None);

        // --- Paragraph style ---
        let mut paragraph_style = ParagraphStyle::default();
        paragraph_style.set_text_align(skia_safe::textlayout::TextAlign::Left);
        paragraph_style.set_max_lines(10);

        let mut builder = ParagraphBuilder::new(&paragraph_style, font_collection);

        let roboto = Typeface::new("Roboto", skia_safe::FontStyle::bold())
        .unwrap_or(Typeface::default());
        let mut style1 = TextStyle::default();
        style1.set_color(Color::BLACK);
        style1.set_font_size(50.0);
        style1.set_typeface(roboto);
        builder.push_style(&style1);
        builder.add_text("Hello, Skia!\n");

        // --- Style 2 (Lato) ---
        let lato = Typeface::new("Lato", skia_safe::FontStyle::italic())
        .unwrap_or(Typeface::default());
        let mut style2 = TextStyle::default();
        style2.set_color(Color::from_rgb(200, 0, 0));
        style2.set_font_size(40.0);
        style2.set_typeface(lato);
        builder.push_style(&style2);
        builder.add_text("Multiple fonts and lines!\n");

        let mut paragraph: Paragraph = builder.build();
        paragraph.layout(width as f32 - 100.0);
        paragraph.paint(canvas, (60.0, 60.0));
    }


    // --- Save PNG ---
    let image = surface.image_snapshot();
    let data = image
        .encode_to_data(skia_safe::EncodedImageFormat::PNG)
        .unwrap();
    let file = File::create("output_paragraph.png").unwrap();
    let mut writer = BufWriter::new(file);
    writer.write_all(data.as_bytes()).unwrap();

    println!("Saved output_paragraph.png");

    Ok(())
}
