use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::book::{BookDocument, ContentMetrics, TextPosition};
use crate::config::ReaderConfig;
use crate::pagination::{
    first_page_for_section, page_for_position, paginate_with_content_height, Page,
};
use crate::storage::Progress;

const NOTICE_DURATION: Duration = Duration::from_millis(2800);
const PRIVACY_READING_RESERVED_ROWS: u16 = 1;

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
}

#[derive(Debug, Default)]
pub(crate) enum Overlay {
    #[default]
    None,
    Percent(PercentNavigator),
    Chapters(ChapterNavigator),
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
pub(crate) struct ChapterNavigator {
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
    OpenChapters,
    OpenSearch,
    OpenMetadata,
    OpenHelp,
    ToggleProgress,
    TogglePrivacy,
    ToggleTheme,
    AddBookmark,
    NextSearch,
    PreviousSearch,
    Move(isize),
    Confirm,
    Cancel,
    Backspace,
    StartFilter,
    Input(char),
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
        Self {
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
        }
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

    pub fn resize(&mut self, width: u16, height: u16) {
        self.viewport = Rect::new(0, 0, width, height);
        self.repaginate();
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
            ReaderAction::OpenChapters => {
                self.overlay = Overlay::Chapters(ChapterNavigator {
                    selection: self.current_section(),
                    ..ChapterNavigator::default()
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
            ReaderAction::TogglePrivacy => {
                self.config.toggle_privacy_mode();
                self.overlay = Overlay::None;
                self.repaginate();
                self.notify(
                    if self.config.privacy_mode {
                        "Privacy mode on"
                    } else {
                        "Privacy mode off"
                    },
                    NotificationKind::Info,
                );
                vec![ReaderEffect::SaveConfig]
            }
            ReaderAction::ToggleTheme => {
                self.config.toggle_theme();
                self.notify(
                    format!("Theme: {}", self.config.theme.label()),
                    NotificationKind::Info,
                );
                vec![ReaderEffect::SaveConfig]
            }
            ReaderAction::AddBookmark => vec![ReaderEffect::AddBookmark {
                label: format!(
                    "Chapter {} · {}",
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
                if let Overlay::Chapters(navigator) = &mut self.overlay {
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
        }
    }

    pub fn visible_chapters(&self, navigator: &ChapterNavigator) -> Vec<usize> {
        visible_chapters(&self.document, navigator)
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
            Overlay::Chapters(navigator) => {
                let visible = visible_chapters(&self.document, navigator);
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
    }

    fn jump_to_position(&mut self, position: TextPosition) {
        self.position = self.metrics.clamp(position);
        self.current_page = page_for_position(&self.pages, self.position);
    }

    fn repaginate(&mut self) {
        self.pages = paginate_for_config(&self.document, self.viewport, &self.config);
        self.current_page = page_for_position(&self.pages, self.position);
    }

    fn move_overlay_selection(&mut self, delta: isize) {
        let Overlay::Chapters(navigator) = &mut self.overlay else {
            return;
        };
        let visible = visible_chapters(&self.document, navigator);
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
            Overlay::Chapters(navigator) => {
                if !navigator.filter_active && !navigator.number_input.is_empty() {
                    let valid_number = navigator
                        .number_input
                        .parse::<usize>()
                        .is_ok_and(|number| (1..=self.document.sections.len()).contains(&number));
                    if !valid_number {
                        if let Overlay::Chapters(navigator) = &mut self.overlay {
                            navigator.error = Some(format!(
                                "Chapter must be between 1 and {}",
                                self.document.sections.len()
                            ));
                        }
                        return Vec::new();
                    }
                }
                let visible = visible_chapters(&self.document, navigator);
                if visible.contains(&navigator.selection) {
                    let selection = navigator.selection;
                    if let Some(page) = first_page_for_section(&self.pages, selection) {
                        self.current_page = page;
                        self.position = self.pages[page].source_start;
                        let title = self.document.sections[selection].title.clone();
                        self.overlay = Overlay::None;
                        self.notify(
                            format!("Chapter {}: {title}", selection + 1),
                            NotificationKind::Success,
                        );
                        return vec![ReaderEffect::SaveProgress(self.current_progress())];
                    }
                }
                if let Overlay::Chapters(navigator) = &mut self.overlay {
                    navigator.error = Some("No matching chapter".to_string());
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
        if let Overlay::Chapters(navigator) = &mut self.overlay {
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
            Overlay::Chapters(navigator) if navigator.filter_active => {
                navigator.filter_query.pop();
                navigator.error = None;
                sync_chapter_selection(&self.document, navigator);
            }
            Overlay::Chapters(navigator) => {
                navigator.number_input.pop();
                navigator.error = None;
                sync_number_selection(self.document.sections.len(), navigator);
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
            Overlay::Chapters(navigator) if navigator.filter_active && !ch.is_control() => {
                navigator.filter_query.push(ch);
                navigator.error = None;
                sync_chapter_selection(&self.document, navigator);
            }
            Overlay::Chapters(navigator) => {
                if ch.is_ascii_digit() {
                    navigator.number_input.push(ch);
                    navigator.error = None;
                    sync_number_selection(self.document.sections.len(), navigator);
                } else {
                    navigator.error = Some("Type a chapter number or press / to filter".into());
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
    let content_height = if config.privacy_mode {
        privacy_content_height(viewport)
    } else {
        viewport.height.saturating_sub(2).max(1)
    };
    paginate_with_content_height(
        &document.sections,
        viewport.width,
        content_height,
        config.text_width,
    )
}

pub(crate) fn privacy_chunks(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(area);
    [chunks[0], chunks[1]]
}

fn privacy_content_height(viewport: Rect) -> u16 {
    privacy_chunks(viewport)[1]
        .height
        .saturating_sub(PRIVACY_READING_RESERVED_ROWS)
        .max(1)
}

fn visible_chapters(document: &BookDocument, navigator: &ChapterNavigator) -> Vec<usize> {
    if !navigator.filter_active || navigator.filter_query.is_empty() {
        return (0..document.sections.len()).collect();
    }
    let needle = navigator.filter_query.to_lowercase();
    document
        .sections
        .iter()
        .enumerate()
        .filter_map(|(index, section)| {
            section
                .title
                .to_lowercase()
                .contains(&needle)
                .then_some(index)
        })
        .collect()
}

fn sync_chapter_selection(document: &BookDocument, navigator: &mut ChapterNavigator) {
    let visible = visible_chapters(document, navigator);
    if !visible.contains(&navigator.selection) {
        navigator.selection = visible.first().copied().unwrap_or(0);
    }
}

fn sync_number_selection(section_count: usize, navigator: &mut ChapterNavigator) {
    if navigator.number_input.is_empty() {
        return;
    }
    match navigator.number_input.parse::<usize>() {
        Ok(number) if (1..=section_count).contains(&number) => {
            navigator.selection = number - 1;
        }
        _ => navigator.error = Some(format!("Chapter must be between 1 and {section_count}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookFormat, BookMetadata, Section};

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
    fn chapter_number_and_filter_select_targets() {
        let mut state = state();
        state.update(ReaderAction::OpenChapters);
        state.update(ReaderAction::Input('2'));
        state.update(ReaderAction::Confirm);
        assert_eq!(state.current_section(), 1);

        state.update(ReaderAction::OpenChapters);
        state.update(ReaderAction::StartFilter);
        for ch in "rust".chars() {
            state.update(ReaderAction::Input(ch));
        }
        let Overlay::Chapters(navigator) = &state.overlay else {
            panic!("chapter overlay expected");
        };
        assert_eq!(state.visible_chapters(navigator), vec![1]);
    }

    #[test]
    fn first_escape_clears_filter_and_second_closes_navigator() {
        let mut state = state();
        state.update(ReaderAction::OpenChapters);
        state.update(ReaderAction::StartFilter);
        state.update(ReaderAction::Input('x'));
        state.update(ReaderAction::Cancel);
        assert!(matches!(
            state.overlay,
            Overlay::Chapters(ChapterNavigator {
                filter_active: false,
                ..
            })
        ));
        state.update(ReaderAction::Cancel);
        assert!(matches!(state.overlay, Overlay::None));
    }

    #[test]
    fn invalid_chapter_number_keeps_navigator_open() {
        let mut state = state();
        state.update(ReaderAction::OpenChapters);
        for ch in "99".chars() {
            state.update(ReaderAction::Input(ch));
        }

        assert!(state.update(ReaderAction::Confirm).is_empty());
        assert!(matches!(
            state.overlay,
            Overlay::Chapters(ChapterNavigator { error: Some(_), .. })
        ));
        assert_eq!(state.current_section(), 0);
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
        state.update(ReaderAction::ToggleTheme);
        state.update(ReaderAction::TogglePrivacy);
        state.update(ReaderAction::TogglePrivacy);
        state.update(ReaderAction::OpenSearch);
        state.update(ReaderAction::Cancel);

        assert_eq!(state.position, before);
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
