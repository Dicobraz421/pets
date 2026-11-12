//! Pets patch: blocks — application elements laid into the text flow, as Zed's editor blocks.
//!
//! A block draws over buffer lines and/or over empty rows reserved after them. The rows are
//! real display rows of the editor (whole multiples of the line height), so scrolling, the
//! scrollbar, hit testing and vertical movement keep one uniform row model: a click on a
//! block lands at the end of its last text row, and the caret steps over reserved rows.
//! What a block shows is up to the application (a table grid over the Markdown of the
//! table, a picture of its own size under a line); the text under a covering block stays
//! editable and is usually made transparent with a [`crate::input::TextDecoration`].
//!
//! Blocks are addressed by buffer line, so the application sets them again after every
//! change of the text ([`InputBaseState::set_blocks`]); until then an edited line keeps the
//! reservation of the line that had its index.
//!
//! Candidate for upstream (longbridge/gpui-kit).
use std::ops::Range;
use std::rc::Rc;

use gpui::{AnyElement, App, Bounds, Context, Pixels, Window};

use super::InputBaseState;
use super::kind::InputModeKind;

/// A block over the buffer lines `lines` and `extra_rows` empty rows after the last of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorBlock {
    /// Application id, passed back to the renderer.
    pub id: usize,
    /// Buffer lines the block belongs to; the rows are reserved after `lines.end - 1`.
    pub lines: Range<usize>,
    /// Empty rows reserved after the last line (each one line height).
    pub extra_rows: usize,
    /// `true` — the block draws over the rows of `lines` too; `false` — only over the
    /// reserved rows.
    pub cover: bool,
}

/// Where the renderer draws: the block's bounds in window coordinates and the top of each of
/// its lines, relative to the block's top (a covering block only).
#[derive(Clone, Debug)]
pub struct EditorBlockContext {
    pub id: usize,
    pub bounds: Bounds<Pixels>,
    pub line_tops: Vec<Pixels>,
    pub line_height: Pixels,
}

/// Builds the element of a block for one frame.
pub type EditorBlockRenderer = Rc<dyn Fn(&EditorBlockContext, &mut Window, &mut App) -> AnyElement>;

/// Blocks of an input and their renderer.
#[derive(Clone, Default)]
pub(crate) struct Blocks {
    pub(crate) blocks: Rc<[EditorBlock]>,
    pub(crate) renderer: Option<EditorBlockRenderer>,
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Replace the blocks of this input. Call again after every text change: blocks are
    /// addressed by buffer line. Blocks with an empty `lines` range or past the text are
    /// ignored.
    pub fn set_blocks(
        &mut self,
        mut blocks: Vec<EditorBlock>,
        renderer: EditorBlockRenderer,
        cx: &mut Context<Self>,
    ) {
        let lines = super::RopeExt::lines_len(&self.text);
        blocks.retain(|b| !b.lines.is_empty() && b.lines.end <= lines);
        blocks.sort_by_key(|b| b.lines.start);
        let mut rows: Vec<(usize, usize)> = blocks
            .iter()
            .filter(|b| b.extra_rows > 0)
            .map(|b| (b.lines.end - 1, b.extra_rows))
            .collect();
        rows.sort_unstable();
        rows.dedup_by(|a, b| {
            if a.0 == b.0 {
                b.1 = b.1.max(a.1);
                true
            } else {
                false
            }
        });
        self.display_map.set_block_rows(Rc::from(rows));
        self.blocks = Blocks {
            blocks: Rc::from(blocks),
            renderer: Some(renderer),
        };
        cx.notify();
    }

    /// Remove all blocks.
    pub fn clear_blocks(&mut self, cx: &mut Context<Self>) {
        self.display_map.set_block_rows(Rc::from([]));
        self.blocks = Blocks::default();
        cx.notify();
    }
}
