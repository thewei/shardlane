//! [INPUT]: render::{markdown, Ctx, MarkdownView, Metrics, Palette,
//! TranscriptSelection}; zero I/O, zero app state.
//! [OUTPUT]: MarkdownViewCache (seq-keyed LRU + source byte budget + single
//! render entry point).
//! [POS]: shared Markdown view cache for herdr-gui `agent_ui`. History Detail
//! (mend=false, static expanded body) and Live Chat (streaming tail mend=true)
//! share one cache and render entry: the parsed structure is roughly 17x the
//! source, so the budget is measured in source_len; a protected streaming seq
//! is never evicted; eviction is safe (the transcript model's source text is
//! the authority).

use std::collections::{HashMap, VecDeque};

use super::render::{markdown, Ctx, MarkdownView, Metrics, Palette, TranscriptSelection};
use gpui::AnyElement;

/// Seq-keyed Markdown view cache: LRU touch + total source byte budget eviction.
#[derive(Default)]
pub struct MarkdownViewCache {
    views: HashMap<i64, MarkdownView>,
    order: VecDeque<i64>,
    bytes: usize,
}

impl MarkdownViewCache {
    pub fn clear(&mut self) {
        self.views.clear();
        self.order.clear();
        self.bytes = 0;
    }

    /// Test-only size probes (eviction contract assertions).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.views.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Single entry point: touch/insert view → set_text (`mend` semantics match
    /// MarkdownView::set_text) → byte accounting → budget eviction → render.
    /// `protect` (e.g. a currently streaming tail seq) is never evicted. Returns
    /// `None` when the seq's empty text has no renderable blocks (caller falls
    /// back to blank).
    #[allow(clippy::too_many_arguments)]
    pub fn render_answer(
        &mut self,
        seq: i64,
        text: &str,
        mend: bool,
        budget_bytes: usize,
        protect: Option<i64>,
        row_key: std::rc::Rc<str>,
        palette: &Palette,
        metrics: Metrics,
        selection: &TranscriptSelection,
        search: Option<super::render::SearchHighlights>,
    ) -> Option<AnyElement> {
        let existed = self.views.contains_key(&seq);
        let view = self.views.entry(seq).or_default();
        if existed {
            self.order.retain(|key| *key != seq);
        }
        self.order.push_back(seq);
        let source_before = view.source_len();
        view.set_text(text, mend);
        let source_after = view.source_len();
        self.bytes = self
            .bytes
            .saturating_sub(source_before)
            .saturating_add(source_after);
        self.evict(budget_bytes, protect);
        let view = self.views.get_mut(&seq)?;
        let mut ctx = Ctx::new(row_key, palette, metrics, selection.clone());
        if let Some(search) = search {
            ctx = ctx.with_search_highlights(search);
        }
        markdown(view, &ctx)
    }

    /// Budget eviction: start from the least-recently-used end; a `protect` hit
    /// moves to the back and is skipped; keep at least one entry (anti-thrash).
    fn evict(&mut self, budget_bytes: usize, protect: Option<i64>) {
        while self.bytes > budget_bytes && self.order.len() > 1 {
            let Some(victim) = self.order.front().copied() else {
                break;
            };
            self.order.pop_front();
            if Some(victim) == protect {
                self.order.push_back(victim);
                continue;
            }
            if let Some(view) = self.views.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(view.source_len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_ui::markdown::render::PaletteSource;

    const TINY_BUDGET: usize = 32;

    /// Test palette: no Default, built from fully transparent Hsla (cache tests
    /// do not validate colors).
    fn test_palette() -> Palette {
        let transparent = gpui::hsla(0.0, 0.0, 0.0, 0.0);
        Palette::from_source(PaletteSource {
            text: transparent,
            secondary: transparent,
            tertiary: transparent,
            ghost: transparent,
            border: transparent,
            inset: transparent,
            overlay: transparent,
            code_text: transparent,
            code_wash: transparent,
            selection: transparent,
            accent: transparent,
            success: transparent,
            danger: transparent,
            is_dark: true,
        })
    }

    fn fill(cache: &mut MarkdownViewCache, seqs: &[i64], text: &str, protect: Option<i64>) {
        let palette = test_palette();
        for &seq in seqs {
            let _ = cache.render_answer(
                seq,
                text,
                false,
                TINY_BUDGET,
                protect,
                std::rc::Rc::from(format!("k{seq}").as_str()),
                &palette,
                Metrics::BODY,
                &TranscriptSelection::default(),
                None,
            );
        }
    }

    #[test]
    fn cache_evicts_lru_under_budget_and_protects_streaming_tail() {
        let mut cache = MarkdownViewCache::default();
        let long = "x".repeat(64);
        fill(&mut cache, &[1, 2, 3], &long, None);
        assert!(
            cache.len() < 3,
            "budget must evict oldest entries, got {}",
            cache.len()
        );

        // A protected seq is never evicted: repeatedly touching other entries
        // pushes the protected one to the LRU oldest end.
        let mut cache = MarkdownViewCache::default();
        fill(&mut cache, &[9], &long, Some(9));
        for round in 0..6 {
            fill(&mut cache, &[100 + round], &long, Some(9));
        }
        assert!(
            cache.views.contains_key(&9),
            "protected streaming tail must survive eviction"
        );
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_renders_settled_text_and_keeps_last_view() {
        let palette = test_palette();
        let mut cache = MarkdownViewCache::default();
        let rendered = cache.render_answer(
            1,
            "# hello",
            false,
            usize::MAX,
            None,
            std::rc::Rc::from("k1"),
            &palette,
            Metrics::BODY,
            &TranscriptSelection::default(),
            None,
        );
        assert!(rendered.is_some(), "non-empty markdown must render");
        // A single entry is never evicted even when over budget (len>1 guard,
        // anti-thrash).
        assert_eq!(cache.len(), 1);
    }
}
