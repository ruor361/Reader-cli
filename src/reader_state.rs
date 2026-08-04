use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use crate::book::{BookDocument, ContentMetrics, TextPosition};
use crate::config::ReaderConfig;
use crate::discreet::{discreet_layout, DiscreetLogState};
use crate::pagination::{
    page_for_position, paginate_with_content_height, paginate_with_exact_content_dimensions, Page,
};
use crate::storage::Progress;

const NOTICE_DURATION: Duration = Duration::from_millis(2800);

#[derive(Debug)]
pub(crate) struct ReaderState {
    pub document: BookDocument,
    pub book_id: String,
    pub metrics: ContentMetrics,
    pub pages: Vec<Page>,
    pub current_page: usize,
    pub position: TextPosition,
    pub viewport: Rect,
    pub overlay: Overlay,
    pub notification: Option<Notification>,
    pub search_results: Vec<TextPosition>,
    pub search_index: usize,
    pub config: ReaderConfig,
    pub hidden: bool,
    pub discreet_log: Option<DiscreetLogState>,
}

#[derive(Debug, Default)]
pub(crate) enum Overlay {
    #[default]
    None,
    Percent(PercentNavigator),
    Contents(ContentsNavigator),
    Search(SearchInput),
    Metadata,
    Help,
}

#[derive(Debug, Default)]
pub(crate) struct PercentNavigator {
    pub input: String,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct ContentsNavigator {
    pub selection: usize,
    pub number_input: String,
    pub filter_active: bool,
    pub filter_query: String,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct SearchInput {
    pub query: String,
    pub error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Notification {
    pub text: String,
    pub kind: NotificationKind,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReaderAction {
    None,
    Exit,
    NextScreen,
    PreviousScreen,
    JumpStart,
    JumpEnd,
    OpenPercent,
    OpenContents,
    OpenSearch,
    OpenMetadata,
    OpenHelp,
    ToggleProgress,
    ToggleDiscreet,
    ToggleHidden,
    AddBookmark,
    NextSearch,
    PreviousSearch,
    Move(isize),
    Confirm,
    Cancel,
    Backspace,
    StartFilter,
    Input(char),
    DiscreetUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReaderEffect {
    SaveProgress(Progress),
    SaveConfig,
    AddBookmark { label: String, progress: Progress },
    Exit,
}

impl ReaderState {
    pub fn new(
        document: BookDocument,
        book_id: String,
        viewport: Rect,
        config: ReaderConfig,
        saved: Progress,
    ) -> Self {
        let metrics = document.content_metrics();
        let position = metrics.clamp(saved.position);
        let pages = paginate_for_config(&document, viewport, &config);
        let current_page = page_for_position(&pages, position);
        let mut state = Self {
            document,
            book_id,
            metrics,
            pages,
            current_page,
            position,
            viewport,
            overlay: Overlay::None,
            notification: None,
            search_results: Vec::new(),
            search_index: 0,
            config,
            hidden: false,
            discreet_log: None,
        };
        state.refresh_discreet_log();
        state
    }

    pub fn current_progress(&self) -> Progress {
        Progress::new(self.position)
    }

    pub fn progress_tenths(&self) -> u16 {
        self.metrics.progress_tenths(self.position)
    }

    pub fn current_section(&self) -> usize {
        self.pages
            .get(self.current_page)
            .map(|page| page.section_index)
            .unwrap_or(self.position.section_index)
            .min(self.document.sections.len().saturating_sub(1))
    }

    pub(crate) fn physical_row_range(&self) -> (usize, usize, usize) {
        let total = self
            .pages
            .iter()
            .map(|page| page.lines.len())
            .sum::<usize>()
            .max(1);
        let start = self
            .pages
            .iter()
            .take(self.current_page)
            .map(|page| page.lines.len())
            .sum::<usize>()
            .saturating_add(1)
            .min(total);
        let visible = self
            .pages
            .get(self.current_page)
            .map(|page| page.lines.len())
            .unwrap_or(1)
            .max(1);
        let end = start.saturating_add(visible.saturating_sub(1)).min(total);
        (start, end, total)
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.viewport = Rect::new(0, 0, width, height);
        if !self.hidden {
            self.repaginate();
        }
    }

    pub fn expire_notification(&mut self, now: Instant) {
        if self
            .notification
            .as_ref()
            .is_some_and(|notification| notification.expires_at <= now)
        {
            self.notification = None;
        }
    }

    pub fn notify(&mut self, text: impl Into<String>, kind: NotificationKind) {
        self.notification = Some(Notification {
            text: text.into(),
            kind,
            expires_at: Instant::now() + NOTICE_DURATION,
        });
    }

    pub fn update(&mut self, action: ReaderAction) -> Vec<ReaderEffect> {
        if self.hidden && !matches!(action, ReaderAction::Exit | ReaderAction::ToggleHidden) {
            return Vec::new();
        }
        if self.config.discreet_mode && !action_is_available_in_discreet(action) {
            self.notify("Unavailable in discreet mode", NotificationKind::Info);
            return Vec::new();
        }

        match action {
            ReaderAction::None => Vec::new(),
            ReaderAction::Exit => vec![ReaderEffect::Exit],
            ReaderAction::NextScreen => self.move_screen(1),
            ReaderAction::PreviousScreen => self.move_screen(-1),
            ReaderAction::JumpStart => self.jump_boundary(false),
            ReaderAction::JumpEnd => self.jump_boundary(true),
            ReaderAction::OpenPercent => {
                self.overlay = Overlay::Percent(PercentNavigator::default());
                Vec::new()
            }
            ReaderAction::OpenContents => {
                self.overlay = Overlay::Contents(ContentsNavigator {
                    selection: contents_selection_for_position(
                        &self.document,
                        &self.metrics,
                        self.position,
                    ),
                    ..ContentsNavigator::default()
                });
                Vec::new()
            }
            ReaderAction::OpenSearch => {
                self.overlay = Overlay::Search(SearchInput::default());
                Vec::new()
            }
            ReaderAction::OpenMetadata => {
                self.overlay = Overlay::Metadata;
                Vec::new()
            }
            ReaderAction::OpenHelp => {
                self.overlay = Overlay::Help;
                Vec::new()
            }
            ReaderAction::ToggleProgress => {
                self.config.toggle_progress();
                self.notify(
                    if self.config.show_progress {
                        "Progress shown"
                    } else {
                        "Progress hidden"
                    },
                    NotificationKind::Info,
                );
                vec![ReaderEffect::SaveConfig]
            }
            ReaderAction::ToggleDiscreet => {
                self.config.toggle_discreet_mode();
                self.overlay = Overlay::None;
                self.repaginate();
                self.notify(
                    if self.config.discreet_mode {
                        "Discreet mode on"
                    } else {
                        "Discreet mode off"
                    },
                    NotificationKind::Info,
                );
                vec![ReaderEffect::SaveConfig]
            }
            ReaderAction::ToggleHidden => {
                self.hidden = !self.hidden;
                if !self.hidden {
                    self.repaginate();
                }
                Vec::new()
            }
            ReaderAction::AddBookmark => vec![ReaderEffect::AddBookmark {
                label: format!(
                    "{} {} · {}",
                    self.document.format.section_label(),
                    self.current_section() + 1,
                    format_tenths(self.progress_tenths())
                ),
                progress: self.current_progress(),
            }],
            ReaderAction::NextSearch => self.jump_search(1),
            ReaderAction::PreviousSearch => self.jump_search(-1),
            ReaderAction::Move(delta) => {
                self.move_overlay_selection(delta);
                Vec::new()
            }
            ReaderAction::Confirm => self.confirm_overlay(),
            ReaderAction::Cancel => {
                self.cancel_overlay();
                Vec::new()
            }
            ReaderAction::Backspace => {
                self.backspace_overlay();
                Vec::new()
            }
            ReaderAction::StartFilter => {
                if let Overlay::Contents(navigator) = &mut self.overlay {
                    navigator.filter_active = true;
                    navigator.filter_query.clear();
                    navigator.number_input.clear();
                    navigator.error = None;
                }
                Vec::new()
            }
            ReaderAction::Input(ch) => {
                self.input_overlay(ch);
                Vec::new()
            }
            ReaderAction::DiscreetUnavailable => {
                self.notify("Unavailable in discreet mode", NotificationKind::Info);
                Vec::new()
            }
        }
    }

    pub fn visible_contents(&self, navigator: &ContentsNavigator) -> Vec<usize> {
        visible_contents(&self.document, navigator)
    }

    fn move_screen(&mut self, delta: isize) -> Vec<ReaderEffect> {
        if !matches!(self.overlay, Overlay::None) {
            return Vec::new();
        }
        let old = self.current_page;
        let last = self.pages.len().saturating_sub(1) as isize;
        let next = (old as isize + delta).clamp(0, last) as usize;
        if next == old {
            return Vec::new();
        }
        self.set_page(next);
        vec![ReaderEffect::SaveProgress(self.current_progress())]
    }

    fn jump_boundary(&mut self, end: bool) -> Vec<ReaderEffect> {
        match &mut self.overlay {
            Overlay::None => {
                let tenths = if end { 1000 } else { 0 };
                self.jump_to_position(self.metrics.position_at_tenths(tenths));
                vec![ReaderEffect::SaveProgress(self.current_progress())]
            }
            Overlay::Contents(navigator) => {
                let visible = visible_contents(&self.document, navigator);
                if let Some(&selection) = if end { visible.last() } else { visible.first() } {
                    navigator.selection = selection;
                    navigator.number_input.clear();
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn set_page(&mut self, page_index: usize) {
        self.current_page = page_index.min(self.pages.len().saturating_sub(1));
        if let Some(page) = self.pages.get(self.current_page) {
            self.position = page.source_start;
        }
        self.refresh_discreet_log();
    }

    fn jump_to_position(&mut self, position: TextPosition) {
        self.position = self.metrics.clamp(position);
        self.current_page = page_for_position(&self.pages, self.position);
        self.refresh_discreet_log();
    }

    fn repaginate(&mut self) {
        self.pages = paginate_for_config(&self.document, self.viewport, &self.config);
        self.current_page = page_for_position(&self.pages, self.position);
        self.refresh_discreet_log();
    }

    fn refresh_discreet_log(&mut self) {
        if !self.config.discreet_mode {
            self.discreet_log = None;
            return;
        }
        let Some(page_start) = self
            .pages
            .get(self.current_page)
            .map(|page| page.source_start)
        else {
            self.discreet_log = None;
            return;
        };
        if self
            .discreet_log
            .as_ref()
            .is_some_and(|log| log.page_start() == page_start)
        {
            return;
        }
        self.discreet_log = Some(DiscreetLogState::new(&self.book_id, page_start));
    }

    fn move_overlay_selection(&mut self, delta: isize) {
        let Overlay::Contents(navigator) = &mut self.overlay else {
            return;
        };
        let visible = visible_contents(&self.document, navigator);
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|index| *index == navigator.selection)
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, visible.len() as isize - 1) as usize;
        navigator.selection = visible[next];
        navigator.number_input.clear();
        navigator.error = None;
    }

    fn confirm_overlay(&mut self) -> Vec<ReaderEffect> {
        match &self.overlay {
            Overlay::Percent(navigator) => match parse_percent_tenths(&navigator.input) {
                Ok(tenths) => {
                    self.jump_to_position(self.metrics.position_at_tenths(tenths));
                    self.overlay = Overlay::None;
                    self.notify(
                        format!("Jumped to {}", format_tenths(tenths)),
                        NotificationKind::Success,
                    );
                    vec![ReaderEffect::SaveProgress(self.current_progress())]
                }
                Err(error) => {
                    if let Overlay::Percent(navigator) = &mut self.overlay {
                        navigator.error = Some(error);
                    }
                    Vec::new()
                }
            },
            Overlay::Contents(navigator) => {
                if !navigator.filter_active && !navigator.number_input.is_empty() {
                    let valid_number = navigator
                        .number_input
                        .parse::<usize>()
                        .is_ok_and(|number| (1..=self.document.contents.len()).contains(&number));
                    if !valid_number {
                        if let Overlay::Contents(navigator) = &mut self.overlay {
                            navigator.error = Some(format!(
                                "Entry must be between 1 and {}",
                                self.document.contents.len()
                            ));
                        }
                        return Vec::new();
                    }
                }
                let visible = visible_contents(&self.document, navigator);
                if visible.contains(&navigator.selection) {
                    let selection = navigator.selection;
                    let entry = self.document.contents[selection].clone();
                    self.jump_to_position(entry.target);
                    self.overlay = Overlay::None;
                    self.notify(
                        format!("Jumped to contents {}: {}", selection + 1, entry.title),
                        NotificationKind::Success,
                    );
                    return vec![ReaderEffect::SaveProgress(self.current_progress())];
                }
                if let Overlay::Contents(navigator) = &mut self.overlay {
                    navigator.error = Some("No matching contents entry".to_string());
                }
                Vec::new()
            }
            Overlay::Search(input) => {
                let query = input.query.trim().to_string();
                if query.is_empty() {
                    if let Overlay::Search(input) = &mut self.overlay {
                        input.error = Some("Enter a search term".to_string());
                    }
                    return Vec::new();
                }
                let results = self.document.search(&query);
                if results.is_empty() {
                    if let Overlay::Search(input) = &mut self.overlay {
                        input.error = Some(format!("No matches for {query:?}"));
                    }
                    return Vec::new();
                }
                self.search_results = results.into_iter().map(|result| result.position).collect();
                self.search_index = 0;
                self.jump_to_position(self.search_results[0]);
                self.overlay = Overlay::None;
                self.notify(
                    format!("Match 1/{} for {query:?}", self.search_results.len()),
                    NotificationKind::Success,
                );
                vec![ReaderEffect::SaveProgress(self.current_progress())]
            }
            Overlay::Metadata | Overlay::Help => {
                self.overlay = Overlay::None;
                Vec::new()
            }
            Overlay::None => Vec::new(),
        }
    }

    fn cancel_overlay(&mut self) {
        if let Overlay::Contents(navigator) = &mut self.overlay {
            if navigator.filter_active {
                navigator.filter_active = false;
                navigator.filter_query.clear();
                navigator.error = None;
                return;
            }
        }
        self.overlay = Overlay::None;
    }

    fn backspace_overlay(&mut self) {
        match &mut self.overlay {
            Overlay::Percent(navigator) => {
                navigator.input.pop();
                navigator.error = None;
            }
            Overlay::Contents(navigator) if navigator.filter_active => {
                navigator.filter_query.pop();
                navigator.error = None;
                sync_contents_selection(&self.document, navigator);
            }
            Overlay::Contents(navigator) => {
                navigator.number_input.pop();
                navigator.error = None;
                sync_number_selection(self.document.contents.len(), navigator);
            }
            Overlay::Search(input) => {
                input.query.pop();
                input.error = None;
            }
            _ => {}
        }
    }

    fn input_overlay(&mut self, ch: char) {
        match &mut self.overlay {
            Overlay::Percent(navigator) => {
                if ch.is_ascii_digit() || ch == '.' {
                    navigator.input.push(ch);
                    navigator.error = None;
                } else {
                    navigator.error = Some("Use a number from 0 to 100".to_string());
                }
            }
            Overlay::Contents(navigator) if navigator.filter_active && !ch.is_control() => {
                navigator.filter_query.push(ch);
                navigator.error = None;
                sync_contents_selection(&self.document, navigator);
            }
            Overlay::Contents(navigator) => {
                if ch.is_ascii_digit() {
                    navigator.number_input.push(ch);
                    navigator.error = None;
                    sync_number_selection(self.document.contents.len(), navigator);
                } else {
                    navigator.error =
                        Some("Type a contents entry number or press / to filter".to_string());
                }
            }
            Overlay::Search(input) if !ch.is_control() => {
                input.query.push(ch);
                input.error = None;
            }
            _ => {}
        }
    }

    fn jump_search(&mut self, delta: isize) -> Vec<ReaderEffect> {
        if self.search_results.is_empty() {
            self.notify("No active search results", NotificationKind::Error);
            return Vec::new();
        }
        let length = self.search_results.len() as isize;
        self.search_index = (self.search_index as isize + delta).rem_euclid(length) as usize;
        self.jump_to_position(self.search_results[self.search_index]);
        self.notify(
            format!(
                "Match {}/{}",
                self.search_index + 1,
                self.search_results.len()
            ),
            NotificationKind::Info,
        );
        vec![ReaderEffect::SaveProgress(self.current_progress())]
    }
}

pub(crate) fn format_tenths(tenths: u16) -> String {
    format!("{}.{:01}%", tenths / 10, tenths % 10)
}

pub(crate) fn parse_percent_tenths(input: &str) -> Result<u16, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a percentage from 0 to 100".to_string());
    }
    let mut parts = input.split('.');
    let whole = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "Enter a percentage from 0 to 100".to_string())?;
    let fraction = match parts.next() {
        None => 0,
        Some("") => 0,
        Some(value) if value.len() == 1 => value
            .parse::<u16>()
            .map_err(|_| "Use at most one decimal place".to_string())?,
        Some(_) => return Err("Use at most one decimal place".to_string()),
    };
    if parts.next().is_some() || whole > 100 || (whole == 100 && fraction > 0) {
        return Err("Percentage must be between 0 and 100".to_string());
    }
    Ok(whole * 10 + fraction)
}

fn paginate_for_config(
    document: &BookDocument,
    viewport: Rect,
    config: &ReaderConfig,
) -> Vec<Page> {
    if config.discreet_mode {
        let layout = discreet_layout(viewport, config.text_width);
        return paginate_with_exact_content_dimensions(
            &document.sections,
            layout.body_width,
            discreet_content_height(viewport),
        );
    }
    paginate_with_content_height(
        &document.sections,
        viewport.width,
        viewport.height.saturating_sub(2).max(1),
        config.text_width,
    )
}

pub(crate) fn discreet_chunks(area: Rect) -> [Rect; 3] {
    let action_height = u16::from(area.height >= 5);
    let desired_reading_height = (area.height / 3).clamp(1, 8);
    let reading_height = desired_reading_height.min(area.height.saturating_sub(action_height));
    let stack_height = area
        .height
        .saturating_sub(reading_height)
        .saturating_sub(action_height);
    let stack = Rect::new(area.x, area.y, area.width, stack_height);
    let reading = Rect::new(
        area.x,
        area.y.saturating_add(stack_height),
        area.width,
        reading_height,
    );
    let action = Rect::new(area.x, reading.bottom(), area.width, action_height);
    [stack, reading, action]
}

fn discreet_content_height(viewport: Rect) -> u16 {
    discreet_chunks(viewport)[1].height.max(1)
}

fn action_is_available_in_discreet(action: ReaderAction) -> bool {
    matches!(
        action,
        ReaderAction::None
            | ReaderAction::Exit
            | ReaderAction::NextScreen
            | ReaderAction::PreviousScreen
            | ReaderAction::JumpStart
            | ReaderAction::JumpEnd
            | ReaderAction::ToggleDiscreet
            | ReaderAction::ToggleHidden
            | ReaderAction::AddBookmark
            | ReaderAction::DiscreetUnavailable
    )
}

fn visible_contents(document: &BookDocument, navigator: &ContentsNavigator) -> Vec<usize> {
    if !navigator.filter_active || navigator.filter_query.is_empty() {
        return (0..document.contents.len()).collect();
    }
    let needle = navigator.filter_query.to_lowercase();
    document
        .contents
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry
                .title
                .to_lowercase()
                .contains(&needle)
                .then_some(index)
        })
        .collect()
}

fn sync_contents_selection(document: &BookDocument, navigator: &mut ContentsNavigator) {
    let visible = visible_contents(document, navigator);
    if !visible.contains(&navigator.selection) {
        navigator.selection = visible.first().copied().unwrap_or(0);
    }
}

fn sync_number_selection(entry_count: usize, navigator: &mut ContentsNavigator) {
    if navigator.number_input.is_empty() {
        return;
    }
    match navigator.number_input.parse::<usize>() {
        Ok(number) if (1..=entry_count).contains(&number) => {
            navigator.selection = number - 1;
        }
        _ => navigator.error = Some(format!("Entry must be between 1 and {entry_count}")),
    }
}

fn contents_selection_for_position(
    document: &BookDocument,
    metrics: &ContentMetrics,
    position: TextPosition,
) -> usize {
    let current_offset = metrics.global_char_offset(position);
    document
        .contents
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let target_offset = metrics.global_char_offset(entry.target);
            (target_offset <= current_offset).then_some((target_offset, index))
        })
        .max()
        .map(|(_, index)| index)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookFormat, BookMetadata, ContentsEntry, ContentsSource, Section};

    fn state() -> ReaderState {
        let sections = vec![
            Section {
                title: "Opening".into(),
                text: "alpha beta gamma delta epsilon".into(),
            },
            Section {
                title: "Second Rust Chapter".into(),
                text: "one two three four five six seven eight".into(),
            },
        ];
        let document = BookDocument {
            id: "book".into(),
            title: "Book".into(),
            path: "book.txt".into(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: "Book".into(),
                author: None,
                language: None,
                publisher: None,
                format: BookFormat::Txt,
                path: "book.txt".into(),
                section_count: sections.len(),
            },
            sections,
            contents: vec![
                ContentsEntry {
                    title: "Opening".into(),
                    depth: 0,
                    target: TextPosition::new(0, 0),
                },
                ContentsEntry {
                    title: "Second Rust Chapter".into(),
                    depth: 1,
                    target: TextPosition::new(1, 4),
                },
            ],
            contents_source: ContentsSource::Document,
        };
        ReaderState::new(
            document,
            "book".into(),
            Rect::new(0, 0, 24, 8),
            ReaderConfig::default(),
            Progress::default(),
        )
    }

    #[test]
    fn parses_percentage_boundaries_and_one_decimal() {
        assert_eq!(parse_percent_tenths("0"), Ok(0));
        assert_eq!(parse_percent_tenths("37.4"), Ok(374));
        assert_eq!(parse_percent_tenths("100"), Ok(1000));
        assert!(parse_percent_tenths("100.1").is_err());
        assert!(parse_percent_tenths("1.25").is_err());
    }

    #[test]
    fn resize_preserves_stable_position() {
        let mut state = state();
        state.jump_to_position(TextPosition::new(1, 12));
        let before = state.position;
        state.resize(80, 24);
        assert_eq!(state.position, before);
    }

    #[test]
    fn percent_confirm_jumps_and_closes_overlay() {
        let mut state = state();
        state.update(ReaderAction::OpenPercent);
        state.update(ReaderAction::Input('5'));
        state.update(ReaderAction::Input('0'));
        let effects = state.update(ReaderAction::Confirm);
        assert!(matches!(state.overlay, Overlay::None));
        assert!(matches!(
            effects.as_slice(),
            [ReaderEffect::SaveProgress(_)]
        ));
        assert!((490..=510).contains(&state.progress_tenths()));
    }

    #[test]
    fn contents_number_and_filter_select_exact_targets() {
        let mut state = state();
        state.update(ReaderAction::OpenContents);
        state.update(ReaderAction::Input('2'));
        state.update(ReaderAction::Confirm);
        assert_eq!(state.position, TextPosition::new(1, 4));

        state.update(ReaderAction::OpenContents);
        state.update(ReaderAction::StartFilter);
        for ch in "rust".chars() {
            state.update(ReaderAction::Input(ch));
        }
        let Overlay::Contents(navigator) = &state.overlay else {
            panic!("contents overlay expected");
        };
        assert_eq!(state.visible_contents(navigator), vec![1]);
    }

    #[test]
    fn contents_open_selects_the_nearest_preceding_target() {
        let mut state = state();
        state.jump_to_position(TextPosition::new(1, 12));

        state.update(ReaderAction::OpenContents);

        let Overlay::Contents(navigator) = &state.overlay else {
            panic!("contents overlay expected");
        };
        assert_eq!(navigator.selection, 1);
    }

    #[test]
    fn first_escape_clears_filter_and_second_closes_navigator() {
        let mut state = state();
        state.update(ReaderAction::OpenContents);
        state.update(ReaderAction::StartFilter);
        state.update(ReaderAction::Input('x'));
        state.update(ReaderAction::Cancel);
        assert!(matches!(
            state.overlay,
            Overlay::Contents(ContentsNavigator {
                filter_active: false,
                ..
            })
        ));
        state.update(ReaderAction::Cancel);
        assert!(matches!(state.overlay, Overlay::None));
    }

    #[test]
    fn invalid_contents_number_keeps_navigator_open() {
        let mut state = state();
        state.update(ReaderAction::OpenContents);
        for ch in "99".chars() {
            state.update(ReaderAction::Input(ch));
        }

        assert!(state.update(ReaderAction::Confirm).is_empty());
        assert!(matches!(
            state.overlay,
            Overlay::Contents(ContentsNavigator { error: Some(_), .. })
        ));
        assert_eq!(state.current_section(), 0);
    }

    #[test]
    fn pdf_navigation_and_bookmarks_use_page_labels() {
        let mut state = state();
        state.document.format = BookFormat::Pdf;
        state.document.metadata.format = BookFormat::Pdf;

        let effects = state.update(ReaderAction::AddBookmark);
        assert!(matches!(
            effects.as_slice(),
            [ReaderEffect::AddBookmark { label, .. }] if label.starts_with("Page 1")
        ));

        state.update(ReaderAction::OpenContents);
        state.update(ReaderAction::Input('9'));
        state.update(ReaderAction::Confirm);
        let Overlay::Contents(navigator) = &state.overlay else {
            panic!("contents overlay expected");
        };
        assert_eq!(
            navigator.error.as_deref(),
            Some("Entry must be between 1 and 2")
        );
    }

    #[test]
    fn non_navigation_toggles_and_bookmarks_preserve_position() {
        let mut state = state();
        state.jump_to_position(TextPosition::new(1, 12));
        let before = state.position;

        assert!(matches!(
            state.update(ReaderAction::AddBookmark).as_slice(),
            [ReaderEffect::AddBookmark { .. }]
        ));
        state.update(ReaderAction::ToggleDiscreet);
        state.update(ReaderAction::ToggleDiscreet);
        state.update(ReaderAction::OpenSearch);
        state.update(ReaderAction::Cancel);

        assert_eq!(state.position, before);
    }

    #[test]
    fn hidden_mode_ignores_mutating_actions_and_restores_exact_state() {
        let mut state = state();
        state.update(ReaderAction::OpenSearch);
        state.update(ReaderAction::Input('x'));
        let position = state.position;

        assert!(state.update(ReaderAction::ToggleHidden).is_empty());
        assert!(state.hidden);
        assert!(state.update(ReaderAction::NextScreen).is_empty());
        assert!(state.update(ReaderAction::ToggleDiscreet).is_empty());
        assert!(state.update(ReaderAction::AddBookmark).is_empty());
        assert_eq!(state.position, position);
        assert!(!state.config.discreet_mode);
        assert!(matches!(
            state.overlay,
            Overlay::Search(SearchInput { ref query, .. }) if query == "x"
        ));

        state.update(ReaderAction::ToggleHidden);
        assert!(!state.hidden);
        assert!(matches!(
            state.overlay,
            Overlay::Search(SearchInput { ref query, .. }) if query == "x"
        ));
    }

    #[test]
    fn discreet_mode_rejects_overlay_and_progress_actions() {
        let mut state = state();
        state.update(ReaderAction::ToggleDiscreet);
        let progress_setting = state.config.show_progress;

        assert!(state.update(ReaderAction::OpenSearch).is_empty());
        assert!(state.update(ReaderAction::ToggleProgress).is_empty());

        assert!(matches!(state.overlay, Overlay::None));
        assert_eq!(state.config.show_progress, progress_setting);
        assert_eq!(
            state
                .notification
                .as_ref()
                .map(|notice| notice.text.as_str()),
            Some("Unavailable in discreet mode")
        );
    }

    #[test]
    fn discreet_log_state_is_stable_until_the_visible_page_changes() {
        let mut state = state();
        state.update(ReaderAction::ToggleDiscreet);
        let first = state.discreet_log.clone();

        state.update(ReaderAction::AddBookmark);
        assert_eq!(state.discreet_log, first);

        state.update(ReaderAction::NextScreen);
        assert_ne!(state.discreet_log, first);
    }

    #[test]
    fn discreet_layout_adapts_the_body_from_one_to_eight_rows() {
        let [stack, reading, action] = discreet_chunks(Rect::new(0, 0, 80, 24));
        assert_eq!(stack.height + reading.height + action.height, 24);
        assert_eq!(reading.height, 8);
        assert_eq!(action.height, 1);

        let [stack, reading, action] = discreet_chunks(Rect::new(0, 0, 40, 8));
        assert_eq!(stack.height + reading.height + action.height, 8);
        assert_eq!(reading.height, 2);
        assert_eq!(action.height, 1);

        let [stack, reading, action] = discreet_chunks(Rect::new(0, 0, 40, 4));
        assert_eq!(stack.height, 3);
        assert_eq!(reading.height, 1);
        assert_eq!(action.height, 0);

        let [stack, reading, action] = discreet_chunks(Rect::new(0, 0, 40, 1));
        assert_eq!(stack.height, 0);
        assert_eq!(reading.height, 1);
        assert_eq!(action.height, 0);

        let [stack, reading, action] = discreet_chunks(Rect::new(0, 0, 40, 0));
        assert_eq!(stack.height, 0);
        assert_eq!(reading.height, 0);
        assert_eq!(action.height, 0);
    }

    #[test]
    fn discreet_text_width_stays_between_twenty_four_and_thirty_two_chinese_characters() {
        let wide = Rect::new(0, 0, 120, 24);
        assert_eq!(discreet_layout(wide, None).body_width, 64);
        assert_eq!(discreet_layout(wide, Some(20)).body_width, 48);
        assert_eq!(discreet_layout(wide, Some(64)).body_width, 64);
        assert_eq!(discreet_layout(wide, Some(100)).body_width, 64);
        assert_eq!(
            discreet_layout(Rect::new(0, 0, 42, 24), None).body_width,
            38
        );
    }

    #[test]
    fn discreet_pagination_uses_the_same_recommended_cjk_width() {
        let text = "中".repeat(100);
        let sections = vec![Section {
            title: "Long Chinese text".into(),
            text,
        }];
        let document = BookDocument {
            id: "cjk".into(),
            title: "CJK".into(),
            path: "cjk.txt".into(),
            format: BookFormat::Txt,
            metadata: BookMetadata {
                title: "CJK".into(),
                author: None,
                language: Some("zh".into()),
                publisher: None,
                format: BookFormat::Txt,
                path: "cjk.txt".into(),
                section_count: 1,
            },
            sections,
            contents: vec![ContentsEntry {
                title: "Long Chinese text".into(),
                depth: 0,
                target: TextPosition::new(0, 0),
            }],
            contents_source: ContentsSource::Generated,
        };
        let mut config = ReaderConfig::default();
        config.discreet_mode = true;
        let mut state = ReaderState::new(
            document,
            "cjk".into(),
            Rect::new(0, 0, 120, 24),
            config,
            Progress::default(),
        );

        assert_eq!(state.pages[0].lines[0].chars().count(), 32);
        assert!(state.pages[0]
            .lines
            .iter()
            .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= 64));

        let position = state.position;
        let wide_total = state.physical_row_range().2;
        state.resize(52, 24);
        assert_eq!(state.position, position);
        assert_ne!(state.physical_row_range().2, wide_total);
        assert!(state.pages[0]
            .lines
            .iter()
            .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= 48));
    }

    #[test]
    fn physical_row_range_is_one_based_continuous_and_counts_blank_rows() {
        let mut state = state();
        state.pages = vec![
            Page {
                index: 0,
                section_index: 0,
                source_start: TextPosition::new(0, 0),
                source_end: TextPosition::new(0, 2),
                lines: vec!["first".into(), String::new(), "third".into()],
            },
            Page {
                index: 1,
                section_index: 0,
                source_start: TextPosition::new(0, 2),
                source_end: TextPosition::new(0, 4),
                lines: vec!["fourth".into(), "fifth".into()],
            },
        ];

        state.current_page = 0;
        assert_eq!(state.physical_row_range(), (1, 3, 5));
        state.current_page = 1;
        assert_eq!(state.physical_row_range(), (4, 5, 5));
    }

    #[test]
    fn entering_last_screen_does_not_mark_book_complete() {
        let mut state = state();
        while state.current_page + 1 < state.pages.len() {
            state.update(ReaderAction::NextScreen);
        }

        assert_eq!(state.position, state.pages.last().unwrap().source_start);
        assert!(state.progress_tenths() < 1000);

        state.update(ReaderAction::JumpEnd);
        assert_eq!(state.progress_tenths(), 1000);
    }
}
