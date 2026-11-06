use super::*;

fn row_text(buffer: &Buffer, row: u16) -> String {
    (1..25)
        .filter_map(|x| buffer.cell((x, row)))
        .map(Cell::symbol)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[test]
fn the_columns_tab_draws_every_column_with_its_kind_and_type() {
    let mut table = orders();
    table.columns.push(column("region", "text"));

    let buffer = draw_state(104, 41, &listing(table));

    assert_eq!(
        ddl_rows(&buffer)[..9],
        [
            // The cursor opens on this row.
            "▌  \u{f084} Schema         shop",
            "   \u{f0ce} Table",
            "     orders_by_customer",
            "",
            "4 columns",
            " \u{f030b} customer_id      uuid",
            " \u{f030b} order_date       date",
            " \u{f030b} order_id       bigint",
            // No glyph outside the primary key.
            "   region           text",
        ]
    );
}

fn index(name: &str, keys: &[&str], method: &str, predicate: Option<&str>) -> Index {
    Index {
        name: name.to_owned(),
        definition: String::new(),
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        unique: false,
        primary: false,
        method: method.to_owned(),
        predicate: predicate.map(str::to_owned),
    }
}

fn indexed() -> Table {
    let mut table = orders();
    table
        .columns
        .extend([column("status", "text"), column("tags", "text[]")]);
    table.indexes = vec![
        index(
            "orders_by_status",
            &["status"],
            "btree",
            Some("(status <> 'done'::text)"),
        ),
        index("orders_by_tag", &["tags"], "gin", None),
    ];
    table
}

fn indexing(table: Table) -> State {
    let mut state = describing(table);
    state.ddl.show(StructureTab::Indexes);
    state
}

#[test]
fn the_indexes_tab_draws_the_primary_key_and_a_block_per_index() {
    let buffer = draw_state(104, 41, &indexing(indexed()));

    assert_eq!(
        ddl_rows(&buffer)[..18],
        [
            "▌  \u{f084} Schema         shop",
            "   \u{f0ce} Table",
            "     orders_by_customer",
            "",
            "\u{f030b} Primary Key",
            "   customer_id      uuid",
            "   order_date       date",
            "   order_id       bigint",
            // Indexes open a group of their own.
            "",
            "\u{f002} orders_by_status",
            "   status           text",
            "   using           btree",
            // Too wide beside its name, so the predicate wraps.
            "   where",
            "     (status <>",
            "     'done'::text)",
            "\u{f002} orders_by_tag",
            "   tags           text[]",
            "   using             gin",
        ]
    );
}

#[test]
fn a_table_with_no_index_says_so_rather_than_showing_an_empty_block() {
    let buffer = draw_state(104, 41, &indexing(orders()));

    assert_eq!(
        ddl_rows(&buffer)[8..11],
        ["", "\u{f002} Indexes", "   none"]
    );
}

#[test]
fn a_title_is_pushed_off_its_corner_by_border_and_not_by_blanks() {
    let buffer = draw(80, 24);
    let border = row(&buffer, 0);
    let lead: String = BORDER_LINE.to_string().repeat(TITLE_INSET);

    assert!(
        border.starts_with(&format!("╭{lead} [1]-Objects ")),
        "the gap reads as the frame running on, got {border:?}"
    );
}

#[test]
fn a_sidebar_wide_enough_names_the_connection_on_the_border() {
    let buffer = draw_state(140, 12, &browsing(&["shop"], 0));
    let border = row(&buffer, 0);

    assert!(
        border.starts_with("╭─ [1]-Objects (local, read-only) "),
        "got {border:?}"
    );
}

#[test]
fn a_writable_posture_wears_its_own_colour_on_the_border() {
    let mut state = browsing(&["shop"], 0);
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lp_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::Writable,
        schema: SchemaState::Loaded(schema_of(&["shop"])),
    });

    let buffer = draw_state(140, 12, &state);
    let border = row(&buffer, 0);
    let at = border
        .find("writable")
        .expect("the border names the posture");
    let fg = |x: usize| {
        buffer
            .cell((u16::try_from(x).expect("inside the border"), 0))
            .expect("inside the border")
            .fg
    };

    assert_eq!(fg(at), Theme::default().palette().warning);
    assert_ne!(
        fg(at),
        fg(at - "local, ".len() - 1),
        "the posture is coloured and the name it qualifies is not"
    );
}

#[test]
fn a_sidebar_too_narrow_for_the_badge_keeps_the_panels_name_alone() {
    let buffer = draw_state(80, 12, &browsing(&["shop"], 0));
    let border = row(&buffer, 0);

    assert!(border.starts_with("╭── [1]-Objects ──"), "got {border:?}");
    assert!(!border.contains("read-only"), "got {border:?}");
}

#[test]
fn a_border_holding_both_puts_the_badge_at_the_name_and_the_marker_at_the_corner() {
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Tables, Search::new("or"));

    // 160 columns leaves a 40-column sidebar; badge, marker, padding and
    // corners need 39.
    let buffer = draw_state(160, 12, &state);
    let border = row(&buffer, 0);
    let (tables, _) = border.split_once('╮').expect("the sidebar's corner");

    assert!(
        tables.starts_with("╭─ [1]-Objects (local, read-only) "),
        "got {tables:?}"
    );
    assert!(tables.ends_with(" /or "), "got {tables:?}");
}

#[test]
fn a_border_that_cannot_hold_both_keeps_the_marker_and_drops_the_badge() {
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Tables, Search::new("or"));

    // 140 columns leaves 35, two short.
    let buffer = draw_state(140, 12, &state);
    let border = row(&buffer, 0);
    let (tables, _) = border.split_once('\u{256e}').expect("the sidebar's corner");

    assert!(
        tables.starts_with("\u{256d}\u{2500}\u{2500} [1]-Objects "),
        "got {tables:?}"
    );
    assert!(!tables.contains("read-only"), "got {tables:?}");
    assert!(tables.ends_with(" /or \u{2500}\u{2500}"), "got {tables:?}");
}

#[test]
fn a_border_with_one_column_to_spare_gives_it_to_the_title_and_not_the_marker() {
    // A 21-column sidebar with `/or` leaves one spare column, which goes to the
    // title.
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Tables, Search::new("or"));

    let buffer = draw_state(84, 12, &state);
    let border = row(&buffer, 0);

    assert!(
        border.starts_with("╭─ [1]-Objects  /or ╮"),
        "got {border:?}"
    );
}

#[test]
fn the_narrowest_sidebar_gives_its_border_to_the_title_alone() {
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Tables, Search::new("or"));

    let buffer = draw_state(40, 12, &state);
    let border = row(&buffer, 0);

    assert!(border.starts_with("╭─ [1]-Objects ╮"), "got {border:?}");
    assert!(!border.contains("/or"), "got {border:?}");
}

#[test]
fn a_border_wide_enough_names_every_tab_and_fades_the_ones_not_drawn() {
    // 152 columns: a 38-column sidebar holds all four full names.
    let buffer = draw_state(152, 41, &describing(orders()));
    let border = row(&buffer, top_border(&buffer, "Columns").1);

    assert!(
        border.contains("Columns - Keys - Indexes - DDL"),
        "got {border:?}"
    );

    let (fg, _, modifier) = style_of_first(&buffer, "ndexes");
    let faint = Theme::default().style(Token::Faint);

    assert_eq!(fg, faint.fg.expect("faint spends a colour"));
    assert_eq!(modifier, faint.add_modifier, "the tab is not the one drawn");
}

#[test]
fn a_sidebar_on_its_usual_width_still_names_every_tab() {
    // A 30-column sidebar: short names, not three dropped tabs.
    let buffer = draw_state(120, 41, &listing(orders()));
    let border = row(&buffer, top_border(&buffer, "Col").1);

    assert!(border.contains("Col - Key - Idx - DDL"), "got {border:?}");
}

#[test]
fn a_border_too_narrow_for_either_strip_keeps_the_tab_it_is_drawing_whole() {
    // The 16-column floor: a clipped strip would name a tab that does not
    // exist.
    let buffer = draw_state(40, 12, &listing(orders()));
    let border = row(&buffer, top_border(&buffer, "Columns").1);

    assert!(border.contains("Columns"), "got {border:?}");
    assert!(!border.contains("DDL"), "got {border:?}");
}

#[test]
fn the_keys_tab_draws_the_key_then_the_references_then_the_rules() {
    let buffer = draw_state(104, 41, &keyed(orders()));

    assert_eq!(
        ddl_rows(&buffer)[..16],
        [
            // The cursor opens on this row.
            "▌  \u{f084} Schema         shop",
            "   \u{f0ce} Table",
            "     orders_by_customer",
            // Between what the panel is about and its shape.
            "",
            // A heading is no cursor stop, so it keeps the bar's column.
            "\u{f030b} Primary Key",
            "   customer_id      uuid",
            "   order_date       date",
            "   order_id       bigint",
            "\u{f030b} Foreign Keys",
            // Too wide for one line, so the target wraps beneath.
            "   customer_id",
            "     shop.customers(id)",
            // Between what the table is and the rules it keeps.
            "",
            "\u{f013} Constraints",
            "   check recent",
            "     CHECK (order_date >",
            "     '2020-01-01')",
        ]
    );
    assert!(ddl_rows(&buffer)[16..].iter().all(String::is_empty));
}

#[test]
fn the_ddl_tab_draws_the_create_statement_keeping_its_indentation() {
    let mut state = describing(orders());
    state.ddl.show(StructureTab::Ddl);
    let buffer = draw_state(104, 41, &state);

    assert_eq!(
        ddl_rows(&buffer)[3..13],
        [
            "",
            "   CREATE TABLE",
            // A name wider than the panel is broken, not dropped.
            "     shop.orders_by_cust",
            "     omer (",
            "       customer_id uuid,",
            "       order_date date,",
            "       order_id bigint,",
            "       CONSTRAINT",
            "         orders_by_custo",
            "         mer_pkey",
        ],
        "a line too long for the panel wraps two in from its own indentation"
    );
}

/// Rows of the 104x41 Ddl body whose first content cell has `Selected`'s
/// background.
fn highlighted_rows(buffer: &Buffer) -> Vec<u16> {
    let highlight = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");

    (DDL_BODY..DDL_END)
        .filter(|&y| buffer.cell((1, y)).is_some_and(|cell| cell.bg == highlight))
        .collect()
}

#[test]
fn every_line_of_a_selected_property_is_highlighted() {
    let mut state = keyed(orders());
    state.focus_on(Panel::Structure);
    // The foreign key's target wraps: one row on two lines. Five rows in.
    state.ddl.cursor.go_to(5, lp_domain::view::ddl_rows(&state));

    let buffer = draw_state(104, 41, &state);

    assert_eq!(
        highlighted_rows(&buffer),
        [26, 27],
        "a wrapped value is one cursor stop, so exactly its own two lines \
         are the selection — not fewer, and not bleeding into the row below"
    );
}

#[test]
fn a_one_line_property_has_exactly_its_own_line_reversed() {
    let mut state = describing(orders());
    state.focus_on(Panel::Structure);
    // The comment, under About, past three columns.
    state.ddl.cursor.go_to(5, lp_domain::view::ddl_rows(&state));

    let buffer = draw_state(104, 41, &state);

    assert_eq!(
        highlighted_rows(&buffer),
        [27],
        "comment fits one line, so exactly it is the selection"
    );
}

#[test]
fn the_ddl_body_reserves_a_column_for_the_selection_bar() {
    let mut state = keyed(orders());
    state.focus_on(Panel::Structure);
    // The foreign key wraps: one cursor stop on two lines.
    state.ddl.cursor.go_to(5, lp_domain::view::ddl_rows(&state));

    let buffer = draw_state(104, 41, &state);
    let bar = |y: u16| buffer.cell((1, y)).expect("inside the panel").symbol();

    assert_eq!(
        highlighted_rows(&buffer),
        [26, 27],
        "the fixture's two lines"
    );
    assert_eq!(bar(26), "▌");
    assert_eq!(bar(27), "▌");
    // 20 and 28 are group blanks; 21, 25 and 29 headings; none pays the bar
    // column.
    for y in [17, 18, 19, 22, 23, 24, 30, 31, 32] {
        assert_eq!(bar(y), " ", "row {y} is a row the cursor is not on");
    }
    assert_eq!(
        bar(29),
        crate::icons::SETTINGS.trim_end(),
        "a heading opens at the margin, in the column a row spends on the bar"
    );
}

#[test]
fn a_section_heading_is_never_drawn_as_the_selection() {
    let mut state = describing(orders());
    state.focus_on(Panel::Structure);
    state.ddl.cursor.to_start();

    let buffer = draw_state(104, 41, &state);
    let highlighted = highlighted_rows(&buffer);

    assert!(
        !highlighted.is_empty(),
        "the first row must be drawn as the selection"
    );
    for row in highlighted {
        let text = row_text(&buffer, row);
        assert!(
            !text.ends_with("Primary Key") && !text.ends_with("Constraints"),
            "row {row} is a heading: {text:?}"
        );
    }
}

#[test]
fn a_ranged_ddl_row_carries_the_bar_and_the_band() {
    let mut state = describing(orders());
    state.focus_on(Panel::Structure);
    state.ddl.cursor.to_start();
    // Row 1 is the table identity, wrapped to two lines; row 2 is outside the
    // range.
    state.ddl.selection = held(0, 1);

    let buffer = draw_state(100, 41, &state);
    let rows = ddl_rows(&buffer);

    assert!(
        rows[1].starts_with('▌'),
        "a row the range covers is marked, as it is in the tree and the grid, got {:?}",
        rows[1]
    );
    let outside = rows
        .iter()
        .find(|line| line.contains("customer_id"))
        .expect("the fixture describes a customer_id column");
    assert!(
        outside.starts_with(' '),
        "and a row outside the range is not, got {outside:?}"
    );
}

#[test]
fn a_selected_block_scrolled_fully_out_of_view_is_not_highlighted() {
    // Checks `details.rs`'s claim that `Paragraph` needs no visibility guard.
    let mut state = describing(orders());
    state.focus_on(Panel::Structure);
    let details = ddl_details(&state);
    state.ddl.cursor.go_to(0, details.row_count());

    // A 14-by-9 Ddl body, which `orders()` overflows.
    let total = crate::details::lay_out(&details, 0, &Marked::none(), 14, None, Theme::default())
        .lines
        .len();
    assert!(
        total > 9,
        "the fixture must overflow the panel or this test proves nothing"
    );
    state.ddl.scroll.to_end(total, 9);

    let highlight = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let buffer = draw_state(40, 20, &state);
    let selected = (9..18).any(|y| buffer.cell((1, y)).expect("inside the panel").bg == highlight);

    assert!(
        !selected,
        "customer_id scrolled off the top must not still be drawn as the selection"
    );
}

#[test]
fn a_selected_block_scrolled_partly_out_of_view_keeps_its_visible_lines_highlighted() {
    let mut state = describing(orders());
    state.focus_on(Panel::Structure);
    let details = ddl_details(&state);
    let last = details.row_count() - 1;
    state.ddl.cursor.go_to(last, details.row_count());

    // `comment` wraps to three lines; scrolled so its third is below the fold.
    let laid = crate::details::lay_out(&details, last, &Marked::none(), 14, None, Theme::default());
    let total = laid.lines.len();
    assert_eq!(laid.selection.lines, 3, "comment wraps to three lines");
    for _ in 0..laid.selection.first - 7 {
        state.ddl.scroll.down(total, 9);
    }

    let highlight = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let buffer = draw_state(40, 20, &state);
    let selected = |y: u16| buffer.cell((1, y)).expect("inside the panel").bg == highlight;

    assert!(!selected(13), "this line belongs to the row above comment");
    assert!(!selected(14), "this line belongs to the row above comment");
    assert!(!selected(15), "this line belongs to the row above comment");
    assert!(
        selected(16),
        "comment's first visible line stays highlighted"
    );
    assert!(
        selected(17),
        "comment's second visible line stays highlighted"
    );
}

#[test]
fn the_key_columns_are_drawn_in_the_tokens_that_mark_them_as_the_key() {
    // The cursor is off the key rows, so they show their own colours.
    let mut state = keyed(orders());
    let details = ddl_details(&state);
    state
        .ddl
        .cursor
        .go_to(details.row_count() - 1, details.row_count());

    let buffer = draw_state(104, 40, &state);
    // Past the bar column: property lines colour their spans.
    let colour = |y: u16| buffer.cell((2, y)).expect("inside the panel").fg;
    let theme = Theme::default();
    let primary = theme
        .style(Token::PrimaryKey)
        .fg
        .expect("the primary key spends a colour");
    let foreign = theme
        .style(Token::ForeignKey)
        .fg
        .expect("a foreign key spends a colour");

    assert_eq!(colour(21), primary, "Primary Key");
    assert_eq!(colour(22), primary, "its column");
    assert_eq!(colour(25), foreign, "Foreign Keys");
    // 28 is the settings group's opening blank.
    assert_eq!(
        colour(29),
        theme
            .style(Token::Section)
            .fg
            .expect("a section heading spends a colour"),
        "Constraints names a group rather than a key"
    );
    assert_eq!(
        colour(30),
        theme
            .style(Token::PropertyName)
            .fg
            .expect("a property name is highlighted"),
        "and its rows keep their own colours rather than the heading's"
    );
}

#[test]
fn a_selected_schema_is_described_by_what_it_holds() {
    let mut state = describing(orders());
    let schema = state.schema().cloned().expect("loaded");
    state.tables.close(&schema);

    let buffer = draw_state(104, 40, &state);

    let rows: Vec<String> = ddl_rows(&buffer)[..4]
        .iter()
        .map(|row| row.trim_start().to_owned())
        .collect();

    assert_eq!(
        rows,
        [
            "▌  \u{f084} Schema         shop",
            "",
            "\u{f013} Contents",
            "tables              1",
        ],
        "no table is selected, so the panel names only the schema"
    );
}

#[test]
fn the_ddl_panel_is_empty_until_something_is_selected() {
    let buffer = draw_state(104, 41, &State::default());

    assert!(
        ddl_rows(&buffer).iter().all(String::is_empty),
        "the Tables panel already says why there is nothing; twice is noise"
    );
}
