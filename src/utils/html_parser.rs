use ego_tree::NodeRef;
use ratatui::prelude::*;
use scraper::{ElementRef, Html, Node};

pub fn html_to_lines(html: &str) -> Vec<Line<'_>> {
    let document = Html::parse_fragment(html);
    let mut converter = HtmlConverter::new();
    converter.process_document(&document);
    converter.finish()
}

struct HtmlConverter {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style_stack: Vec<Style>,
}

impl HtmlConverter {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_spans: Vec::new(),
            style_stack: vec![Style::default()],
        }
    }

    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, new_style: Style) {
        let combined = self.current_style().patch(new_style);
        self.style_stack.push(combined);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn add_text(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let style = self.current_style();

        // Handle line breaks in text
        let lines: Vec<&str> = text.split('\n').collect();
        for (i, line_text) in lines.iter().enumerate() {
            if i > 0 {
                self.finish_line();
            }

            if !line_text.trim().is_empty() {
                self.current_spans
                    .push(Span::styled(line_text.to_string(), style));
            }
        }
    }

    fn finish_line(&mut self) {
        if !self.current_spans.is_empty() {
            let spans = std::mem::take(&mut self.current_spans);
            self.lines.push(Line::from(spans));
        } else {
            // Empty line
            self.lines.push(Line::from(""));
        }
    }

    fn process_element(&mut self, element: ElementRef) {
        let tag_name = element.value().name();

        // Determine style for this element
        let element_style = match tag_name {
            "b" | "strong" => Style::default().add_modifier(Modifier::BOLD),
            "i" | "em" => Style::default().add_modifier(Modifier::ITALIC),
            "u" => Style::default().add_modifier(Modifier::UNDERLINED),
            "code" => Style::default().bg(Color::DarkGray).fg(Color::White),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
            "a" => Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED),
            "span" => {
                // Handle inline styles
                self.parse_inline_style(element.value().attr("style").unwrap_or(""))
            }
            _ => Style::default(),
        };

        // Handle block elements that should create new lines
        let is_block = matches!(
            tag_name,
            "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "br"
        );

        if is_block && !self.current_spans.is_empty() {
            self.finish_line();
        }

        if tag_name == "br" {
            self.finish_line();
            return;
        }

        // Push new style
        self.push_style(element_style);

        // Process children
        for child in element.children() {
            self.process_node(child);
        }

        // Pop style
        self.pop_style();

        // Add space after inline elements that typically need separation
        if matches!(tag_name, "a") {
            self.current_spans.push(Span::raw(" "));
        }

        // Finish line for block elements
        if is_block && tag_name != "br" {
            self.finish_line();
        }
    }

    fn process_node(&mut self, node: NodeRef<Node>) {
        match node.value() {
            Node::Text(text) => {
                self.add_text(&text.text);
            }
            Node::Element(_) => {
                if let Some(element) = ElementRef::wrap(node) {
                    self.process_element(element);
                }
            }
            _ => {
                // Process children for other node types
                for child in node.children() {
                    self.process_node(child);
                }
            }
        }
    }

    fn process_document(&mut self, document: &Html) {
        for node in document.tree.root().children() {
            self.process_node(node);
        }
    }

    fn parse_inline_style(&self, style_attr: &str) -> Style {
        let mut style = Style::default();

        for declaration in style_attr.split(';') {
            let parts: Vec<&str> = declaration.split(':').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                match parts[0] {
                    "color" => {
                        if let Some(color) = self.parse_color(parts[1]) {
                            style = style.fg(color);
                        }
                    }
                    "background-color" => {
                        if let Some(color) = self.parse_color(parts[1]) {
                            style = style.bg(color);
                        }
                    }
                    "font-weight" => {
                        if parts[1] == "bold" || parts[1].parse::<u32>().unwrap_or(400) >= 700 {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                    }
                    "font-style" => {
                        if parts[1] == "italic" {
                            style = style.add_modifier(Modifier::ITALIC);
                        }
                    }
                    "text-decoration" => {
                        if parts[1].contains("underline") {
                            style = style.add_modifier(Modifier::UNDERLINED);
                        }
                    }
                    _ => {}
                }
            }
        }

        style
    }

    fn parse_color(&self, color_str: &str) -> Option<Color> {
        let color_str = color_str.trim().to_lowercase();

        // Handle hex colors
        if let Some(color) = color_str.strip_prefix('#')
            && let Ok(hex) = u32::from_str_radix(color, 16)
        {
            let r = ((hex >> 16) & 0xFF) as u8;
            let g = ((hex >> 8) & 0xFF) as u8;
            let b = (hex & 0xFF) as u8;
            return Some(Color::Rgb(r, g, b));
        }

        // Handle named colors
        match color_str.as_str() {
            "red" => Some(Color::Red),
            "green" => Some(Color::Green),
            "blue" => Some(Color::Blue),
            "yellow" => Some(Color::Yellow),
            "cyan" => Some(Color::Cyan),
            "magenta" => Some(Color::Magenta),
            "white" => Some(Color::White),
            "black" => Some(Color::Black),
            "gray" | "grey" => Some(Color::Gray),
            "darkgray" | "darkgrey" => Some(Color::DarkGray),
            "lightred" => Some(Color::LightRed),
            "lightgreen" => Some(Color::LightGreen),
            "lightblue" => Some(Color::LightBlue),
            "lightyellow" => Some(Color::LightYellow),
            "lightcyan" => Some(Color::LightCyan),
            "lightmagenta" => Some(Color::LightMagenta),
            _ => None,
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.current_spans.is_empty() {
            self.finish_line();
        }

        // Remove empty lines at the end
        while let Some(last_line) = self.lines.last() {
            if last_line.spans.is_empty()
                || (last_line.spans.len() == 1 && last_line.spans[0].content.trim().is_empty())
            {
                self.lines.pop();
            } else {
                break;
            }
        }

        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_html() {
        let html = "<p>Hello <b>world</b>!</p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_multiple_paragraphs() {
        let html = "<p>First paragraph</p><p>Second paragraph</p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_line_breaks() {
        let html = "Line 1<br>Line 2<br>Line 3";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_styled_text() {
        let html = r#"<span style="color: red; font-weight: bold;">Red bold text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());
        let span = &lines[0].spans[0];
        assert_eq!(span.style.fg, Some(Color::Red));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }
}

// Example usage:
//
// use ratatui::prelude::*;
// use ratatui::widgets::{Block, Borders, Paragraph};
//
// fn render_html_content(html: &str) -> Paragraph {
//     let lines = html_to_lines(html);
//     Paragraph::new(lines)
//         .block(Block::default().borders(Borders::ALL).title("HTML Content"))
//         .wrap(ratatui::widgets::Wrap { trim: true })
// }
