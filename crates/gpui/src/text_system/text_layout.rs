use crate::{OverflowWrap, Pixels, ResolvedDirection, TextAlign, TextRun, UnicodeBidi, WordBreak};

/// The paragraph base-direction policy supplied to a text backend.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ParagraphDirection {
    /// Determine paragraph direction from its source text.
    #[default]
    Auto,
    /// Use an explicit left-to-right paragraph base direction.
    LeftToRight,
    /// Use an explicit right-to-left paragraph base direction.
    RightToLeft,
}

impl From<ResolvedDirection> for ParagraphDirection {
    fn from(direction: ResolvedDirection) -> Self {
        match direction {
            ResolvedDirection::LeftToRight => Self::LeftToRight,
            ResolvedDirection::RightToLeft => Self::RightToLeft,
        }
    }
}

/// Direction, wrapping, and alignment options for a complete text document.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TextLayoutOptions {
    /// Optional soft-wrap width.
    pub wrap_width: Option<Pixels>,
    /// Optional maximum number of visual rows.
    pub line_clamp: Option<usize>,
    /// Where lines may break within a word.
    pub word_break: WordBreak,
    /// Whether a word too long for its line may break.
    pub overflow_wrap: OverflowWrap,
    /// Width used for horizontal alignment, independently of wrapping.
    pub alignment_width: Option<Pixels>,
    /// Horizontal text alignment.
    pub text_align: TextAlign,
    /// Paragraph base-direction policy.
    pub direction: ParagraphDirection,
    /// Bidirectional behavior applied to the complete document.
    pub unicode_bidi: UnicodeBidi,
}

/// Complete input for one backend-owned text document layout.
#[derive(Clone, Copy, Debug)]
pub struct TextLayoutRequest<'a> {
    /// UTF-8 source text.
    pub text: &'a str,
    /// Font size shared by all style runs.
    pub font_size: Pixels,
    /// Complete shaping and paint styles covering `text`.
    pub runs: &'a [TextRun],
    /// Wrapping, alignment, and bidirectional layout options.
    pub options: TextLayoutOptions,
}
