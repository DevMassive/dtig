use crate::app::{App, FocusArea};
use crate::git::FileType;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn ui(frame: &mut Frame, app: &App) {
    let screen_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
        .split(frame.area());

    let left_chunks = Layout::default()
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(screen_chunks[0]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title("Commit Message");
    let input = Paragraph::new(app.commit_message.as_str())
        .style(match app.focus {
            FocusArea::Commit => Style::default().fg(Color::Yellow),
            _ => Style::default(),
        })
        .block(input_block);
    frame.render_widget(input, left_chunks[0]);

    if let FocusArea::Commit = app.focus {
        frame.set_cursor_position((
            left_chunks[0].x + app.commit_message.len() as u16 + 1,
            left_chunks[0].y + 1,
        ));
    }

    let file_chunks = Layout::default()
        .constraints(
            [
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ]
            .as_ref(),
        )
        .split(left_chunks[1]);

    let staged_items: Vec<ListItem> = app
        .status
        .staged
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let mut style = Style::default();
            if let FocusArea::Files = app.focus {
                if matches!(app.selected_file_type, FileType::Staged)
                    && app.selected_file_index == i
                {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let staged_list =
        List::new(staged_items).block(Block::default().borders(Borders::ALL).title("Staged"));
    frame.render_widget(staged_list, file_chunks[0]);

    let not_staged_items: Vec<ListItem> = app
        .status
        .not_staged
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let mut style = Style::default();
            if let FocusArea::Files = app.focus {
                if matches!(app.selected_file_type, FileType::NotStaged)
                    && app.selected_file_index == i
                {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let not_staged_list = List::new(not_staged_items)
        .block(Block::default().borders(Borders::ALL).title("Not Staged"));
    frame.render_widget(not_staged_list, file_chunks[1]);

    let untracked_items: Vec<ListItem> = app
        .status
        .untracked
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let mut style = Style::default();
            if let FocusArea::Files = app.focus {
                if matches!(app.selected_file_type, FileType::Untracked)
                    && app.selected_file_index == i
                {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let untracked_list =
        List::new(untracked_items).block(Block::default().borders(Borders::ALL).title("Untracked"));
    frame.render_widget(untracked_list, file_chunks[2]);

    let diff_area = screen_chunks[1];

    let mut diff_text_spans = Vec::new();

    for (i, line) in app.diff.lines().enumerate() {
        let mut style = Style::default();
        if let FocusArea::Diff = app.focus {
            if i == app.diff_selected_line {
                style = style.add_modifier(Modifier::REVERSED);
            }
        }
        diff_text_spans.push(Line::from(Span::styled(line, style)));
    }

    let diff_view = Paragraph::new(diff_text_spans)
        .block(Block::default().borders(Borders::ALL).title("Diff"))
        .scroll((app.diff_scroll, 0));
    frame.render_widget(diff_view, diff_area);

    if let FocusArea::Diff = app.focus {
        let cursor_x = diff_area.x + 1;
        let cursor_y =
            diff_area.y + 1 + (app.diff_selected_line as u16).saturating_sub(app.diff_scroll);

        if cursor_y > diff_area.y && cursor_y < diff_area.y + diff_area.height.saturating_sub(1) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
