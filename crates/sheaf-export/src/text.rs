//! Text: picks a face per character, shapes with rustybuzz and breaks lines greedily
//! at spaces, and inside a word only if the word alone is wider than the line.
//!
//! Text runs left to right; right-to-left scripts are set in logical order.

use std::collections::VecDeque;
use std::ops::Range;

use krilla::text::{Font, GlyphId, KrillaGlyph};
use rustybuzz::UnicodeBuffer;

use crate::Error;
use crate::blocks::Span;
use crate::fonts::{FontFile, Fonts};

/// A face of [`Faces`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceId(usize);

pub struct Face {
    file: FontFile,
    pub font: Font,
    /// Above the baseline, per em.
    pub ascent: f32,
    /// Below the baseline, per em, positive.
    pub descent: f32,
}

/// The fonts of a document, loaded.
pub struct Faces {
    faces: Vec<Face>,
    pub regular: FaceId,
    pub semibold: FaceId,
    pub bold: FaceId,
    pub extra_bold: FaceId,
    pub mono: FaceId,
    fallbacks: Vec<FaceId>,
}

impl Faces {
    pub fn load(fonts: &Fonts) -> Result<Faces, Error> {
        let mut faces = Vec::new();
        let mut add = |file: &FontFile, name: &'static str| -> Result<FaceId, Error> {
            let font = Font::new(file.data().clone(), file.index()).ok_or(Error::Font(name))?;
            let face = rustybuzz::Face::from_slice(file.data().as_ref(), file.index())
                .ok_or(Error::Font(name))?;
            let em = f32::from(u16::try_from(face.units_per_em()).unwrap_or(1000));
            faces.push(Face {
                file: file.clone(),
                font,
                ascent: f32::from(face.ascender()) / em,
                descent: -f32::from(face.descender()) / em,
            });
            Ok(FaceId(faces.len() - 1))
        };
        let regular = add(&fonts.regular, "regular")?;
        let semibold = add(&fonts.semibold, "semibold")?;
        let bold = add(&fonts.bold, "bold")?;
        let extra_bold = add(&fonts.extra_bold, "extra bold")?;
        let mono = match &fonts.mono {
            Some(file) => add(file, "mono")?,
            None => regular,
        };
        // A fallback that cannot be read is left out; the text faces still work.
        let fallbacks = fonts
            .fallbacks
            .iter()
            .filter_map(|file| add(file, "fallback").ok())
            .collect();
        Ok(Faces {
            faces,
            regular,
            semibold,
            bold,
            extra_bold,
            mono,
            fallbacks,
        })
    }

    pub fn get(&self, id: FaceId) -> &Face {
        &self.faces[id.0]
    }

    fn parse(&self, id: FaceId) -> Option<rustybuzz::Face<'_>> {
        let file = &self.get(id).file;
        rustybuzz::Face::from_slice(file.data().as_ref(), file.index())
    }
}

/// How the text of a paragraph, heading or cell is set, before the spans' own styles.
#[derive(Clone, Copy, Debug)]
pub struct Base {
    pub size: f32,
    pub face: FaceId,
    /// Line height as a multiple of `size`.
    pub leading: f32,
}

/// Part of a line in one face and from one span.
#[derive(Clone, Debug)]
pub struct Frag {
    /// From the start of the line.
    pub x: f32,
    pub width: f32,
    pub face: FaceId,
    pub size: f32,
    /// Index of the span it comes from.
    pub span: usize,
    pub text: String,
    /// Text ranges relative to `text`.
    pub glyphs: Vec<KrillaGlyph>,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub width: f32,
    pub height: f32,
    /// From the top of the line to the baseline.
    pub baseline: f32,
    pub frags: Vec<Frag>,
}

/// Shaped text without a break inside, except after its trailing spaces.
#[derive(Clone, Debug)]
struct Piece {
    face: FaceId,
    size: f32,
    span: usize,
    text: String,
    glyphs: Vec<KrillaGlyph>,
    width: f32,
    /// Width of the spaces at its end, which do not count at the end of a line.
    trailing: f32,
    /// A line may end after it.
    breakable: bool,
    /// A line must end after it.
    newline: bool,
}

/// The face and size `span` is set in on top of `base`.
fn face_of(faces: &Faces, base: &Base, span: &Span) -> (FaceId, f32) {
    if span.style.code {
        return (faces.mono, base.size * 0.9);
    }
    let face = if span.style.bold && (base.face == faces.regular || base.face == faces.semibold) {
        faces.bold
    } else {
        base.face
    };
    (face, base.size)
}

/// Characters that belong to the character before them, whatever the face.
fn joins_previous(c: char) -> bool {
    matches!(c,
        '\u{200D}' | '\u{FE00}'..='\u{FE0F}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{20E3}'
        | '\u{E0020}'..='\u{E007F}' | '\u{0300}'..='\u{036F}')
}

/// Splits `text` into runs of the face that has its characters: `primary` where it
/// can, else the first fallback that can.
fn runs(faces: &Faces, primary: FaceId, text: &str) -> Vec<(Range<usize>, FaceId)> {
    let parsed: Vec<(FaceId, Option<rustybuzz::Face<'_>>)> = std::iter::once(primary)
        .chain(faces.fallbacks.iter().copied())
        .map(|id| (id, faces.parse(id)))
        .collect();
    let mut runs: Vec<(Range<usize>, FaceId)> = Vec::new();
    for (start, c) in text.char_indices() {
        let end = start + c.len_utf8();
        let face = match runs.last() {
            Some((_, face)) if c.is_whitespace() || joins_previous(c) => *face,
            _ => parsed
                .iter()
                .find(|(_, face)| {
                    face.as_ref()
                        .is_some_and(|face| face.glyph_index(c).is_some())
                })
                .map_or(primary, |(id, _)| *id),
        };
        match runs.last_mut() {
            Some((range, last)) if *last == face => range.end = end,
            _ => runs.push((start..end, face)),
        }
    }
    runs
}

/// Shapes `text` in one face; text ranges relative to `text`.
fn shape(faces: &Faces, face: FaceId, text: &str) -> Vec<KrillaGlyph> {
    let Some(parsed) = faces.parse(face) else {
        return Vec::new();
    };
    let em = f32::from(u16::try_from(parsed.units_per_em()).unwrap_or(1000));
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    buffer.set_direction(rustybuzz::Direction::LeftToRight);
    let shaped = rustybuzz::shape(&parsed, &[], buffer);
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();
    let mut glyphs = Vec::with_capacity(infos.len());
    for (index, (info, position)) in infos.iter().zip(positions).enumerate() {
        let start = info.cluster as usize;
        let end = infos[index..]
            .iter()
            .map(|next| next.cluster as usize)
            .find(|cluster| *cluster != start)
            .unwrap_or(text.len());
        glyphs.push(KrillaGlyph::new(
            GlyphId::new(info.glyph_id),
            position.x_advance as f32 / em,
            position.x_offset as f32 / em,
            position.y_offset as f32 / em,
            position.y_advance as f32 / em,
            start..end,
            None,
        ));
    }
    glyphs
}

/// The pieces of `spans`, in order.
fn pieces(faces: &Faces, base: &Base, spans: &[Span]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for (index, span) in spans.iter().enumerate() {
        let (primary, size) = face_of(faces, base, span);
        let mut lines = span.text.split('\n').peekable();
        while let Some(line) = lines.next() {
            let text = line.replace('\t', "    ");
            let before = pieces.len();
            for (range, face) in runs(faces, primary, &text) {
                let run = &text[range];
                split_run(
                    &mut pieces,
                    run,
                    &shape(faces, face, run),
                    face,
                    size,
                    index,
                );
            }
            if lines.peek().is_some() {
                let added = pieces.len() > before;
                match pieces.last_mut() {
                    Some(last) if added => last.newline = true,
                    _ => pieces.push(Piece {
                        face: primary,
                        size,
                        span: index,
                        text: String::new(),
                        glyphs: Vec::new(),
                        width: 0.,
                        trailing: 0.,
                        breakable: true,
                        newline: true,
                    }),
                }
            }
        }
    }
    pieces
}

/// Cuts a shaped run after each group of spaces.
fn split_run(
    pieces: &mut Vec<Piece>,
    run: &str,
    glyphs: &[KrillaGlyph],
    face: FaceId,
    size: f32,
    span: usize,
) {
    let is_space = |glyph: &KrillaGlyph| {
        run.get(glyph.text_range.clone())
            .is_some_and(|text| text.chars().all(|c| c == ' ' || c == '\u{2009}'))
    };
    let mut start = 0;
    for index in 0..glyphs.len() {
        let ends = is_space(&glyphs[index]) && glyphs.get(index + 1).is_none_or(|g| !is_space(g));
        if ends || index + 1 == glyphs.len() {
            let part = &glyphs[start..=index];
            let trailing: f32 = part
                .iter()
                .rev()
                .take_while(|glyph| is_space(glyph))
                .map(|glyph| glyph.x_advance * size)
                .sum();
            pieces.push(piece(run, part, face, size, span, trailing, ends));
            start = index + 1;
        }
    }
}

fn piece(
    run: &str,
    glyphs: &[KrillaGlyph],
    face: FaceId,
    size: f32,
    span: usize,
    trailing: f32,
    breakable: bool,
) -> Piece {
    let from = glyphs
        .iter()
        .map(|glyph| glyph.text_range.start)
        .min()
        .unwrap_or(0);
    let to = glyphs
        .iter()
        .map(|glyph| glyph.text_range.end)
        .max()
        .unwrap_or(from);
    let glyphs: Vec<KrillaGlyph> = glyphs
        .iter()
        .map(|glyph| KrillaGlyph {
            text_range: glyph.text_range.start - from..glyph.text_range.end - from,
            ..glyph.clone()
        })
        .collect();
    Piece {
        face,
        size,
        span,
        text: run[from..to].to_owned(),
        width: glyphs.iter().map(|glyph| glyph.x_advance * size).sum(),
        glyphs,
        trailing,
        breakable,
        newline: false,
    }
}

/// Splits `piece` before the first cluster that does not fit into `room`; the head
/// keeps at least one cluster. `None` if nothing is left for the tail.
fn split_piece(piece: &Piece, room: f32) -> Option<(Piece, Piece)> {
    let mut width = 0.;
    let mut cut = None;
    for (index, glyph) in piece.glyphs.iter().enumerate() {
        let starts_cluster = index > 0 && piece.glyphs[index - 1].text_range != glyph.text_range;
        if starts_cluster && width + glyph.x_advance * piece.size > room {
            cut = Some(index);
            break;
        }
        width += glyph.x_advance * piece.size;
    }
    let cut = cut?;
    let head = piece_from(piece, &piece.glyphs[..cut], 0., true);
    let tail = piece_from(piece, &piece.glyphs[cut..], piece.trailing, piece.breakable);
    Some((
        head,
        Piece {
            newline: piece.newline,
            ..tail
        },
    ))
}

fn piece_from(piece: &Piece, glyphs: &[KrillaGlyph], trailing: f32, breakable: bool) -> Piece {
    self::piece(
        &piece.text,
        glyphs,
        piece.face,
        piece.size,
        piece.span,
        trailing,
        breakable,
    )
}

/// Breaks `spans` into lines no wider than `width`.
pub fn break_lines(faces: &Faces, base: &Base, spans: &[Span], width: f32) -> Vec<Line> {
    let mut queue: VecDeque<Piece> = pieces(faces, base, spans).into();
    let mut lines = Vec::new();
    let mut line: Vec<Piece> = Vec::new();
    let mut used = 0.;
    while let Some(first) = queue.pop_front() {
        // The pieces up to the next place a line may end.
        let mut word = vec![first];
        while word
            .last()
            .is_some_and(|piece| !piece.breakable && !piece.newline)
        {
            match queue.pop_front() {
                Some(piece) => word.push(piece),
                None => break,
            }
        }
        let word_width: f32 = word.iter().map(|piece| piece.width).sum();
        let trailing = word.last().map_or(0., |piece| piece.trailing);
        if !line.is_empty() && used + word_width - trailing > width {
            lines.push(finish(faces, base, std::mem::take(&mut line)));
            used = 0.;
        }
        if line.is_empty() && word_width - trailing > width {
            // Wider than a line on its own: cut it where the line is full.
            let mut taken = 0.;
            let mut rest: VecDeque<Piece> = word.into();
            while let Some(piece) = rest.pop_front() {
                if taken + piece.width - piece.trailing <= width {
                    taken += piece.width;
                    line.push(piece);
                    continue;
                }
                match split_piece(&piece, width - taken) {
                    Some((head, tail)) if line.is_empty() || taken + head.width <= width => {
                        line.push(head);
                        rest.push_front(tail);
                    }
                    // Not even one character fits; take it anyway, or this never ends.
                    _ if line.is_empty() => line.push(piece),
                    _ => rest.push_front(piece),
                }
                break;
            }
            lines.push(finish(faces, base, std::mem::take(&mut line)));
            used = 0.;
            for piece in rest.into_iter().rev() {
                queue.push_front(piece);
            }
            continue;
        }
        let newline = word.last().is_some_and(|piece| piece.newline);
        used += word_width;
        line.extend(word);
        if newline {
            lines.push(finish(faces, base, std::mem::take(&mut line)));
            used = 0.;
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(finish(faces, base, line));
    }
    lines
}

/// The width of `spans` on one line, and of its widest word.
pub fn measure(faces: &Faces, base: &Base, spans: &[Span]) -> (f32, f32) {
    let mut total: f32 = 0.;
    let mut line: f32 = 0.;
    let mut word: f32 = 0.;
    let mut widest: f32 = 0.;
    for piece in pieces(faces, base, spans) {
        line += piece.width;
        word += piece.width - piece.trailing;
        if piece.breakable || piece.newline {
            widest = widest.max(word);
            word = 0.;
        }
        if piece.newline {
            total = total.max(line);
            line = 0.;
        }
    }
    (total.max(line), widest.max(word))
}

/// Places the pieces of a line one after the other and joins neighbours of the same
/// face and span.
fn finish(faces: &Faces, base: &Base, pieces: Vec<Piece>) -> Line {
    let face = faces.get(base.face);
    let height = base.size * base.leading;
    let content = (face.ascent + face.descent) * base.size;
    let baseline = (height - content) / 2. + face.ascent * base.size;
    let mut frags: Vec<Frag> = Vec::new();
    let mut x = 0.;
    for piece in pieces {
        match frags.last_mut() {
            Some(frag)
                if frag.face == piece.face
                    && frag.span == piece.span
                    && frag.size == piece.size =>
            {
                let offset = frag.text.len();
                frag.text.push_str(&piece.text);
                frag.glyphs
                    .extend(piece.glyphs.into_iter().map(|glyph| KrillaGlyph {
                        text_range: glyph.text_range.start + offset..glyph.text_range.end + offset,
                        ..glyph
                    }));
                frag.width += piece.width;
            }
            _ => frags.push(Frag {
                x,
                width: piece.width,
                face: piece.face,
                size: piece.size,
                span: piece.span,
                text: piece.text,
                glyphs: piece.glyphs,
            }),
        }
        x += piece.width;
    }
    let trailing_space = frags.last().map_or(0., |frag| {
        frag.glyphs
            .iter()
            .rev()
            .take_while(|glyph| {
                frag.text
                    .get(glyph.text_range.clone())
                    .is_some_and(|text| text.trim().is_empty())
            })
            .map(|glyph| glyph.x_advance * frag.size)
            .sum::<f32>()
    });
    Line {
        width: x - trailing_space,
        height,
        baseline,
        frags,
    }
}
