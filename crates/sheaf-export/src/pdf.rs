//! Laid-out pages to a PDF file with krilla. Text is drawn as glyphs with their text,
//! so it can be selected and searched.

use krilla::Document;
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;

use crate::Error;
use crate::layout::{ACCENT, FAINT, Item, Laid, Rgb};
use crate::text::Faces;

/// Slant of the text set in italics, as Inter comes without an italic face.
const SLANT: f32 = 0.2;
/// The control point distance of a quarter circle drawn as a cubic curve.
const KAPPA: f32 = 0.552_284_8;

pub fn write(laid: &Laid, faces: &Faces, size: (f32, f32), title: &str) -> Result<Vec<u8>, Error> {
    let mut document = Document::new();
    let settings = PageSettings::from_wh(size.0, size.1).ok_or(Error::PageSize)?;
    for page in &laid.pages {
        let mut started = document.start_page_with(settings.clone());
        let mut surface = started.surface();
        for item in &page.items {
            draw(&mut surface, item, faces);
        }
        surface.finish();
        for link in &page.links {
            let Some(rect) = Rect::from_xywh(link.x, link.y, link.width, link.height) else {
                continue;
            };
            let target = Target::Action(Action::Link(LinkAction::new(link.uri.clone())));
            started.add_annotation(Annotation::new_link(
                LinkAnnotation::new(rect, target),
                Some(link.uri.clone()),
            ));
        }
        started.finish();
    }
    let mut outline = Outline::new();
    for heading in &laid.headings {
        let destination = XyzDestination::new(heading.page, Point::from_xy(0., heading.y));
        outline.push_child(OutlineNode::new(heading.text.clone(), destination));
    }
    document.set_outline(outline);
    let mut metadata = Metadata::new().creator("Sheaf".to_owned());
    if !title.is_empty() {
        metadata = metadata.title(title.to_owned());
    }
    document.set_metadata(metadata);
    document
        .finish()
        .map_err(|err| Error::Pdf(format!("{err:?}")))
}

fn fill(color: Rgb) -> Fill {
    Fill {
        paint: rgb::Color::new(color.0, color.1, color.2).into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    }
}

fn draw(surface: &mut Surface<'_>, item: &Item, faces: &Faces) {
    match item {
        Item::Text {
            x,
            baseline,
            face,
            size,
            color,
            italic,
            text,
            glyphs,
        } => {
            surface.set_stroke(None);
            surface.set_fill(Some(fill(*color)));
            if *italic {
                // Leans right around the baseline: x' = x - SLANT·(y - baseline).
                surface.push_transform(&Transform::from_row(
                    1.,
                    0.,
                    -SLANT,
                    1.,
                    SLANT * baseline,
                    0.,
                ));
            }
            surface.draw_glyphs(
                Point::from_xy(*x, *baseline),
                glyphs,
                faces.get(*face).font.clone(),
                text,
                *size,
                false,
            );
            if *italic {
                surface.pop();
            }
        }
        Item::Rect {
            x,
            y,
            width,
            height,
            color,
        } => {
            let Some(rect) = Rect::from_xywh(*x, *y, *width, *height) else {
                return;
            };
            let mut path = PathBuilder::new();
            path.push_rect(rect);
            if let Some(path) = path.finish() {
                surface.set_stroke(None);
                surface.set_fill(Some(fill(*color)));
                surface.draw_path(&path);
            }
        }
        Item::Line {
            from,
            to,
            width,
            color,
        } => {
            let mut path = PathBuilder::new();
            path.move_to(from.0, from.1);
            path.line_to(to.0, to.1);
            if let Some(path) = path.finish() {
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: rgb::Color::new(color.0, color.1, color.2).into(),
                    width: *width,
                    ..Stroke::default()
                }));
                surface.draw_path(&path);
                surface.set_stroke(None);
            }
        }
        Item::Dot { x, y, radius } => {
            if let Some(path) = circle(*x, *y, *radius) {
                surface.set_stroke(None);
                surface.set_fill(Some(fill(crate::layout::TEXT)));
                surface.draw_path(&path);
            }
        }
        Item::Check {
            x,
            y,
            radius,
            ticked,
        } => {
            let Some(ring) = circle(*x, *y, *radius) else {
                return;
            };
            if *ticked {
                surface.set_stroke(None);
                surface.set_fill(Some(fill(ACCENT)));
                surface.draw_path(&ring);
                let mut tick = PathBuilder::new();
                tick.move_to(x - radius * 0.45, y + radius * 0.02);
                tick.line_to(x - radius * 0.12, y + radius * 0.35);
                tick.line_to(x + radius * 0.48, y - radius * 0.3);
                if let Some(tick) = tick.finish() {
                    surface.set_fill(None);
                    surface.set_stroke(Some(Stroke {
                        paint: rgb::Color::new(255, 255, 255).into(),
                        width: radius * 0.28,
                        ..Stroke::default()
                    }));
                    surface.draw_path(&tick);
                }
            } else {
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: rgb::Color::new(FAINT.0, FAINT.1, FAINT.2).into(),
                    width: radius * 0.16,
                    ..Stroke::default()
                }));
                surface.draw_path(&ring);
            }
            surface.set_stroke(None);
        }
        Item::Image {
            x,
            y,
            width,
            height,
            image,
        } => {
            let Some(size) = Size::from_wh(*width, *height) else {
                return;
            };
            surface.push_transform(&Transform::from_translate(*x, *y));
            surface.draw_image(image.clone(), size);
            surface.pop();
        }
    }
}

fn circle(x: f32, y: f32, radius: f32) -> Option<krilla::geom::Path> {
    let k = radius * KAPPA;
    let mut path = PathBuilder::new();
    path.move_to(x + radius, y);
    path.cubic_to(x + radius, y + k, x + k, y + radius, x, y + radius);
    path.cubic_to(x - k, y + radius, x - radius, y + k, x - radius, y);
    path.cubic_to(x - radius, y - k, x - k, y - radius, x, y - radius);
    path.cubic_to(x + k, y - radius, x + radius, y - k, x + radius, y);
    path.close();
    path.finish()
}
