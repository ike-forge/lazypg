//! The Structure panel's state: a cursor over rows, a viewport over the lines they
//! wrapped to, and a search.

use crate::search::Search;
use crate::selection::Selection;
use crate::viewport::{Cursor, Scroll};

/// A relation's tabs, plus `Summary` for a schema or database, which has no
/// columns to list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StructureTab {
    #[default]
    Columns,
    /// Every constraint: the key, references both ways, unique, check and
    /// exclusion.
    Keys,
    Indexes,
    Ddl,
    Summary,
}

impl StructureTab {
    /// A relation's, in strip order.
    pub const ALL: [Self; 4] = [Self::Columns, Self::Keys, Self::Indexes, Self::Ddl];

    /// No key per tab: `]` and `[` walk them.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Columns => Self::Keys,
            Self::Keys => Self::Indexes,
            Self::Indexes => Self::Ddl,
            Self::Ddl => Self::Columns,
            Self::Summary => Self::Summary,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Columns => Self::Ddl,
            Self::Keys => Self::Columns,
            Self::Indexes => Self::Keys,
            Self::Ddl => Self::Indexes,
            Self::Summary => Self::Summary,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Columns => "Columns",
            Self::Keys => "Keys",
            Self::Indexes => "Indexes",
            Self::Ddl => "DDL",
            // The panel's own name: a schema's or database's one tab is the
            // whole panel.
            Self::Summary => "Structure",
        }
    }

    /// Three letters each: the strip then fits a 29-column sidebar, a
    /// 116-column terminal.
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Columns => "Col",
            Self::Keys => "Key",
            Self::Indexes => "Idx",
            Self::Ddl => "DDL",
            Self::Summary => "Structure",
        }
    }
}

/// Not in `update`'s `LISTS`: cursor and viewport count different units, so it
/// follows a span.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdlPanel {
    pub cursor: Cursor,
    pub scroll: Scroll,
    pub selection: Selection,
    /// Private: a tab change owes a cursor and viewport reset.
    tab: StructureTab,
    /// Private: setting it owes a repair.
    search: Option<Search>,
}

impl DdlPanel {
    #[must_use]
    pub const fn tab(&self) -> StructureTab {
        self.tab
    }

    /// A cursor or range carried across would address rows of another tab.
    /// Re-showing the drawn tab is not a change.
    pub fn show(&mut self, tab: StructureTab) {
        if self.tab == tab {
            return;
        }

        self.tab = tab;
        self.cursor.to_start();
        self.scroll.to_start();
        self.selection.clear();
    }

    #[must_use]
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// Not public: [`State::apply_search`](crate::state::State::apply_search)
    /// owes the repair.
    pub(crate) fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Range, Ranges};

    #[test]
    fn a_panel_opens_on_the_columns() {
        assert_eq!(DdlPanel::default().tab(), StructureTab::Columns);
    }

    #[test]
    fn walking_the_tabs_wraps_and_stepping_back_undoes_it() {
        for tab in StructureTab::ALL {
            assert_eq!(tab.next().previous(), tab);
        }

        assert_eq!(
            std::iter::successors(Some(StructureTab::ALL[0]), |tab| Some(tab.next()))
                .take(StructureTab::ALL.len())
                .collect::<Vec<_>>(),
            StructureTab::ALL,
            "one press per tab returns to where it started"
        );
    }

    #[test]
    fn showing_a_tab_draws_it() {
        let mut panel = DdlPanel::default();

        panel.show(StructureTab::Keys);

        assert_eq!(panel.tab(), StructureTab::Keys);
    }

    #[test]
    fn a_tab_change_puts_the_cursor_and_the_viewport_back_at_the_top() {
        let mut panel = DdlPanel::default();
        panel.cursor.go_to(4, 8);
        panel.scroll.down(40, 10);

        panel.show(StructureTab::Keys);

        assert_eq!(panel.cursor.index(), 0);
        assert_eq!(panel.scroll.offset(), 0);
    }

    #[test]
    fn showing_the_tab_already_drawn_leaves_the_cursor_and_the_viewport_alone() {
        // Pressed to arrive, not to start over.
        let mut panel = DdlPanel::default();
        panel.cursor.go_to(4, 8);
        panel.scroll.down(40, 10);

        panel.show(StructureTab::Columns);

        assert_eq!(panel.cursor.index(), 4);
        assert_eq!(panel.scroll.offset(), 1);
    }

    #[test]
    fn a_tab_change_clears_the_range() {
        let mut panel = DdlPanel::default();
        panel.selection.start(1);
        panel.selection.stop(3);

        panel.show(StructureTab::Keys);

        assert!(!panel.selection.is_active());
    }

    #[test]
    fn arriving_at_the_tab_already_drawn_leaves_the_range_alone() {
        let mut panel = DdlPanel::default();
        panel.selection.start(1);
        panel.selection.stop(3);

        panel.show(StructureTab::Columns);

        assert_eq!(panel.selection.ranges(0), Ranges::of(Range::new(1, 3)));
    }

    #[test]
    fn a_tab_change_leaves_the_search_where_it_was() {
        // Applied to the panel, not a tab.
        let mut panel = DdlPanel::default();
        panel.set_search(Search::new("id"));

        panel.show(StructureTab::Keys);

        assert_eq!(panel.search().map(Search::as_str), Some("id"));
    }
}
