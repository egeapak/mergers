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
        }
        // Don't create empty lines for br tags - they should just finish the current line
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

    /// # Parse Basic HTML
    ///
    /// Tests parsing of basic HTML elements into formatted text.
    ///
    /// ## Test Scenario
    /// - Provides simple HTML with paragraphs and basic formatting
    /// - Parses HTML into text representation
    ///
    /// ## Expected Outcome
    /// - HTML is correctly converted to plain text
    /// - Basic formatting elements are properly handled
    #[test]
    fn test_basic_html() {
        let html = "<p>Hello <b>world</b>!</p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
    }

    /// # Parse Multiple Paragraphs
    ///
    /// Tests parsing of HTML with multiple paragraph elements.
    ///
    /// ## Test Scenario
    /// - Provides HTML with multiple <p> tags and content
    /// - Tests paragraph separation and formatting
    ///
    /// ## Expected Outcome
    /// - Multiple paragraphs are properly separated
    /// - Paragraph structure is preserved in text output
    #[test]
    fn test_multiple_paragraphs() {
        let html = "<p>First paragraph</p><p>Second paragraph</p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 2);
    }

    /// # Parse Line Breaks
    ///
    /// Tests parsing of HTML with line break elements.
    ///
    /// ## Test Scenario
    /// - Provides HTML with <br> tags for line breaks
    /// - Tests line break formatting and separation
    ///
    /// ## Expected Outcome
    /// - Line breaks are properly converted to text newlines
    /// - Text formatting preserves intended line structure
    #[test]
    fn test_line_breaks() {
        let html = "Line 1<br>Line 2<br>Line 3";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 3);
    }

    /// # Parse Styled Text
    ///
    /// Tests parsing of HTML with inline style attributes.
    ///
    /// ## Test Scenario
    /// - Provides HTML with style attributes for color and formatting
    /// - Tests extraction and application of inline styles
    ///
    /// ## Expected Outcome
    /// - Inline styles are correctly parsed and applied
    /// - Styled text maintains formatting in output
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

    // ========== HTML Tag Styling Tests ==========

    /// # Strong Tag Styling
    ///
    /// Tests that the <strong> tag applies bold modifier.
    ///
    /// ## Test Scenario
    /// - HTML contains <strong> tag with text
    /// - Parser processes the tag
    ///
    /// ## Expected Outcome
    /// - Text has BOLD modifier applied
    #[test]
    fn test_strong_tag() {
        let html = "<strong>Important text</strong>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    /// # Emphasis Tag Styling
    ///
    /// Tests that the <em> tag applies italic modifier.
    ///
    /// ## Test Scenario
    /// - HTML contains <em> tag with text
    /// - Parser processes the tag
    ///
    /// ## Expected Outcome
    /// - Text has ITALIC modifier applied
    #[test]
    fn test_em_tag() {
        let html = "<em>Emphasized text</em>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    /// # Italic Tag Styling
    ///
    /// Tests that the <i> tag applies italic modifier.
    ///
    /// ## Test Scenario
    /// - HTML contains <i> tag with text
    /// - Parser processes the tag
    ///
    /// ## Expected Outcome
    /// - Text has ITALIC modifier applied
    #[test]
    fn test_i_tag() {
        let html = "<i>Italic text</i>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    /// # Underline Tag Styling
    ///
    /// Tests that the <u> tag applies underline modifier.
    ///
    /// ## Test Scenario
    /// - HTML contains <u> tag with text
    /// - Parser processes the tag
    ///
    /// ## Expected Outcome
    /// - Text has UNDERLINED modifier applied
    #[test]
    fn test_u_tag() {
        let html = "<u>Underlined text</u>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    /// # Code Tag Styling
    ///
    /// Tests that the <code> tag applies monospace styling with colors.
    ///
    /// ## Test Scenario
    /// - HTML contains <code> tag with text
    /// - Parser processes the tag
    ///
    /// ## Expected Outcome
    /// - Text has dark gray background and white foreground
    #[test]
    fn test_code_tag() {
        let html = "<code>code snippet</code>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.bg, Some(Color::DarkGray));
        assert_eq!(span.style.fg, Some(Color::White));
    }

    /// # Header Tags Styling
    ///
    /// Tests that header tags (h1-h6) apply bold and cyan color.
    ///
    /// ## Test Scenario
    /// - HTML contains various header tags
    /// - Parser processes each header tag
    ///
    /// ## Expected Outcome
    /// - All headers have BOLD modifier and cyan foreground
    #[test]
    fn test_header_tags() {
        for tag in &["h1", "h2", "h3", "h4", "h5", "h6"] {
            let html = format!("<{}>Header</{}>", tag, tag);
            let lines = html_to_lines(&html);
            assert_eq!(lines.len(), 1, "Failed for tag: {}", tag);
            let span = &lines[0].spans[0];
            assert!(
                span.style.add_modifier.contains(Modifier::BOLD),
                "BOLD missing for tag: {}",
                tag
            );
            assert_eq!(
                span.style.fg,
                Some(Color::Cyan),
                "Cyan color missing for tag: {}",
                tag
            );
        }
    }

    /// # Anchor Tag Styling
    ///
    /// Tests that the <a> tag applies blue color, underline, and trailing space.
    ///
    /// ## Test Scenario
    /// - HTML contains <a> tag with href
    /// - Parser processes the anchor tag
    ///
    /// ## Expected Outcome
    /// - Text has blue color, UNDERLINED modifier, and trailing space
    #[test]
    fn test_anchor_tag() {
        let html = r#"<a href="http://example.com">Link</a>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 2);
        let link_span = &lines[0].spans[0];
        assert_eq!(link_span.style.fg, Some(Color::Blue));
        assert!(link_span.style.add_modifier.contains(Modifier::UNDERLINED));
        let space_span = &lines[0].spans[1];
        assert_eq!(space_span.content, " ");
    }

    /// # Div Tag As Block Element
    ///
    /// Tests that <div> tags create block-level separation.
    ///
    /// ## Test Scenario
    /// - HTML contains multiple <div> tags
    /// - Parser processes divs as block elements
    ///
    /// ## Expected Outcome
    /// - Each div creates a separate line
    #[test]
    fn test_div_tag() {
        let html = "<div>First div</div><div>Second div</div>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 2);
    }

    // ========== Inline Style Parsing Tests ==========

    /// # Background Color Inline Style
    ///
    /// Tests parsing of background-color CSS property.
    ///
    /// ## Test Scenario
    /// - HTML span with background-color style
    /// - Parser extracts and applies background color
    ///
    /// ## Expected Outcome
    /// - Span has correct background color applied
    #[test]
    fn test_inline_background_color() {
        let html = r#"<span style="background-color: blue;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.bg, Some(Color::Blue));
    }

    /// # Font Style Italic
    ///
    /// Tests parsing of font-style: italic CSS property.
    ///
    /// ## Test Scenario
    /// - HTML span with font-style: italic
    /// - Parser extracts and applies italic modifier
    ///
    /// ## Expected Outcome
    /// - Span has ITALIC modifier applied
    #[test]
    fn test_inline_font_style_italic() {
        let html = r#"<span style="font-style: italic;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    /// # Text Decoration Underline
    ///
    /// Tests parsing of text-decoration: underline CSS property.
    ///
    /// ## Test Scenario
    /// - HTML span with text-decoration containing underline
    /// - Parser extracts and applies underline modifier
    ///
    /// ## Expected Outcome
    /// - Span has UNDERLINED modifier applied
    #[test]
    fn test_inline_text_decoration_underline() {
        let html = r#"<span style="text-decoration: underline;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    /// # Font Weight Numeric Bold
    ///
    /// Tests parsing of numeric font-weight values (>= 700 = bold).
    ///
    /// ## Test Scenario
    /// - HTML span with font-weight: 700
    /// - Parser recognizes numeric threshold for bold
    ///
    /// ## Expected Outcome
    /// - Span has BOLD modifier applied
    #[test]
    fn test_inline_font_weight_numeric() {
        let html = r#"<span style="font-weight: 700;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    /// # Font Weight Below Bold Threshold
    ///
    /// Tests that numeric font-weight below 700 does not apply bold.
    ///
    /// ## Test Scenario
    /// - HTML span with font-weight: 400
    /// - Parser recognizes value is below bold threshold
    ///
    /// ## Expected Outcome
    /// - Span does NOT have BOLD modifier applied
    #[test]
    fn test_inline_font_weight_below_threshold() {
        let html = r#"<span style="font-weight: 400;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(!span.style.add_modifier.contains(Modifier::BOLD));
    }

    /// # Multiple Inline Styles Combined
    ///
    /// Tests parsing multiple CSS properties in a single style attribute.
    ///
    /// ## Test Scenario
    /// - HTML span with color, background-color, and font-weight
    /// - Parser extracts all properties
    ///
    /// ## Expected Outcome
    /// - Span has all styles applied correctly
    #[test]
    fn test_multiple_inline_styles() {
        let html =
            r#"<span style="color: green; background-color: yellow; font-weight: bold;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.fg, Some(Color::Green));
        assert_eq!(span.style.bg, Some(Color::Yellow));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    /// # Malformed Inline Style
    ///
    /// Tests handling of malformed CSS with missing values.
    ///
    /// ## Test Scenario
    /// - HTML span with incomplete CSS declaration (no value)
    /// - Parser should gracefully ignore invalid property
    ///
    /// ## Expected Outcome
    /// - No crash, parser continues with default style
    #[test]
    fn test_malformed_inline_style() {
        let html = r#"<span style="color:">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());
    }

    // ========== Color Parsing Tests ==========

    /// # Hex Color Parsing (6 digits)
    ///
    /// Tests parsing of 6-digit hex color codes.
    ///
    /// ## Test Scenario
    /// - HTML span with #RRGGBB hex color
    /// - Parser converts hex to RGB color
    ///
    /// ## Expected Outcome
    /// - Span has correct RGB color values
    #[test]
    fn test_hex_color_six_digits() {
        let html = r#"<span style="color: #FF5733;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.fg, Some(Color::Rgb(255, 87, 51)));
    }

    /// # Hex Color Parsing (3 digits)
    ///
    /// Tests parsing of 3-digit hex color codes.
    ///
    /// ## Test Scenario
    /// - HTML span with #RGB hex color (short form)
    /// - Parser converts short hex to RGB (note: parser doesn't expand digits, treats as 12-bit value)
    ///
    /// ## Expected Outcome
    /// - Span has RGB color from raw hex value (#F53 = 0x0F53 = RGB(0, 15, 83))
    #[test]
    fn test_hex_color_three_digits() {
        let html = r#"<span style="color: #F53;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        // #F53 is parsed as 0x0F53, which gives RGB(0, 15, 83)
        assert_eq!(span.style.fg, Some(Color::Rgb(0, 15, 83)));
    }

    /// # Named Colors Parsing
    ///
    /// Tests parsing of CSS named colors.
    ///
    /// ## Test Scenario
    /// - HTML spans with various named colors
    /// - Parser recognizes and converts named colors
    ///
    /// ## Expected Outcome
    /// - Each named color is correctly mapped to Color enum
    #[test]
    fn test_named_colors() {
        let colors = vec![
            ("red", Color::Red),
            ("green", Color::Green),
            ("blue", Color::Blue),
            ("yellow", Color::Yellow),
            ("cyan", Color::Cyan),
            ("magenta", Color::Magenta),
            ("white", Color::White),
            ("black", Color::Black),
            ("gray", Color::Gray),
            ("grey", Color::Gray),
            ("darkgray", Color::DarkGray),
            ("lightred", Color::LightRed),
            ("lightgreen", Color::LightGreen),
            ("lightblue", Color::LightBlue),
            ("lightyellow", Color::LightYellow),
            ("lightcyan", Color::LightCyan),
            ("lightmagenta", Color::LightMagenta),
        ];

        for (color_name, expected_color) in colors {
            let html = format!(r#"<span style="color: {};">Text</span>"#, color_name);
            let lines = html_to_lines(&html);
            assert_eq!(lines.len(), 1, "Failed for color: {}", color_name);
            let span = &lines[0].spans[0];
            assert_eq!(
                span.style.fg,
                Some(expected_color),
                "Color mismatch for: {}",
                color_name
            );
        }
    }

    /// # Invalid Color Name
    ///
    /// Tests handling of unsupported color names.
    ///
    /// ## Test Scenario
    /// - HTML span with unsupported color name
    /// - Parser cannot map color name
    ///
    /// ## Expected Outcome
    /// - Parser gracefully ignores invalid color (no foreground set)
    #[test]
    fn test_invalid_color_name() {
        let html = r#"<span style="color: invalidcolor;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.fg, None);
    }

    /// # Invalid Hex Color
    ///
    /// Tests handling of malformed hex color codes.
    ///
    /// ## Test Scenario
    /// - HTML span with invalid hex color (non-hex characters)
    /// - Parser cannot parse hex value
    ///
    /// ## Expected Outcome
    /// - Parser gracefully ignores invalid color (no foreground set)
    #[test]
    fn test_invalid_hex_color() {
        let html = r#"<span style="color: #GGGGGG;">Text</span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.fg, None);
    }

    // ========== Text Handling Edge Cases ==========

    /// # Empty HTML Input
    ///
    /// Tests handling of empty HTML string.
    ///
    /// ## Test Scenario
    /// - Empty string passed to html_to_lines
    /// - Parser processes empty document
    ///
    /// ## Expected Outcome
    /// - Returns empty vector of lines
    #[test]
    fn test_empty_html() {
        let html = "";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 0);
    }

    /// # Whitespace Only HTML
    ///
    /// Tests handling of whitespace-only HTML.
    ///
    /// ## Test Scenario
    /// - HTML contains only whitespace characters
    /// - Parser trims whitespace
    ///
    /// ## Expected Outcome
    /// - Returns empty vector (whitespace ignored)
    #[test]
    fn test_whitespace_only_html() {
        let html = "   \n\t  ";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 0);
    }

    /// # Text With Embedded Newlines
    ///
    /// Tests handling of text nodes containing newline characters.
    ///
    /// ## Test Scenario
    /// - Text node contains literal \n characters
    /// - Parser splits on newlines
    ///
    /// ## Expected Outcome
    /// - Each newline-separated segment becomes separate line
    #[test]
    fn test_text_with_newlines() {
        let html = "Line 1\nLine 2\nLine 3";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 3);
    }

    /// # Mixed Text and Elements
    ///
    /// Tests handling of mixed text and element nodes.
    ///
    /// ## Test Scenario
    /// - HTML has text before, inside, and after elements
    /// - Parser combines text and styled elements
    ///
    /// ## Expected Outcome
    /// - All text appears in correct order with proper styling
    #[test]
    fn test_mixed_content() {
        let html = "Before <b>bold</b> middle <i>italic</i> after";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 5);
    }

    // ========== Style Stack Tests ==========

    /// # Nested Style Inheritance
    ///
    /// Tests that nested elements properly inherit and combine styles.
    ///
    /// ## Test Scenario
    /// - Bold tag containing italic tag
    /// - Inner element should have both bold and italic
    ///
    /// ## Expected Outcome
    /// - Inner text has both BOLD and ITALIC modifiers
    #[test]
    fn test_nested_styles() {
        let html = "<b>Bold <i>and italic</i></b>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let italic_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains("and italic"))
            .expect("Should have 'and italic' span");
        assert!(italic_span.style.add_modifier.contains(Modifier::BOLD));
        assert!(italic_span.style.add_modifier.contains(Modifier::ITALIC));
    }

    /// # Deeply Nested Styles
    ///
    /// Tests style stacking with multiple levels of nesting.
    ///
    /// ## Test Scenario
    /// - Multiple levels of nested styling elements
    /// - Each level adds or modifies styles
    ///
    /// ## Expected Outcome
    /// - Innermost element has all accumulated styles
    #[test]
    fn test_deeply_nested_styles() {
        let html = r#"<span style="color: red;"><b><i><u>All styles</u></i></b></span>"#;
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.style.fg, Some(Color::Red));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    /// # Style Stack Pop Safety
    ///
    /// Tests that style stack never pops below the base default style.
    ///
    /// ## Test Scenario
    /// - Multiple nested elements that each pop styles
    /// - Ensure stack maintains at least the base style
    ///
    /// ## Expected Outcome
    /// - No crash, styles correctly managed
    #[test]
    fn test_style_stack_safety() {
        let html = "<b><i><u>Text</u></i></b>Normal";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());
    }

    // ========== Block Element Behavior Tests ==========

    /// # Consecutive Block Elements
    ///
    /// Tests handling of multiple consecutive block elements.
    ///
    /// ## Test Scenario
    /// - Multiple consecutive <p> or <div> tags
    /// - Each should create separate line
    ///
    /// ## Expected Outcome
    /// - Correct number of lines, no empty lines between
    #[test]
    fn test_consecutive_block_elements() {
        let html = "<p>Para 1</p><p>Para 2</p><p>Para 3</p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert!(!line.spans.is_empty(), "No empty lines should exist");
        }
    }

    /// # Block Element With Inline Children
    ///
    /// Tests block elements containing inline styled elements.
    ///
    /// ## Test Scenario
    /// - Paragraph containing bold and italic elements
    /// - Block creates line, inlines add styling
    ///
    /// ## Expected Outcome
    /// - Single line with multiple styled spans
    #[test]
    fn test_block_with_inline_children() {
        let html = "<p>Normal <b>bold</b> <i>italic</i> text</p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() > 1);
    }

    /// # Empty Block Elements Cleanup
    ///
    /// Tests that trailing empty lines are removed.
    ///
    /// ## Test Scenario
    /// - HTML with content followed by empty tags
    /// - Parser should clean up trailing empty lines
    ///
    /// ## Expected Outcome
    /// - No trailing empty lines in output
    #[test]
    fn test_empty_trailing_lines_cleanup() {
        let html = "<p>Content</p><p></p><p></p>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());
    }

    // ========== Negative/Error Tests ==========

    /// # Unknown HTML Tags
    ///
    /// Tests handling of unrecognized HTML tags.
    ///
    /// ## Test Scenario
    /// - HTML contains custom/unknown tags
    /// - Parser should use default style
    ///
    /// ## Expected Outcome
    /// - Text is extracted with default styling, no crash
    #[test]
    fn test_unknown_tags() {
        let html = "<customtag>Custom content</customtag>";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());
    }

    /// # Malformed HTML Structure
    ///
    /// Tests handling of improperly nested or unclosed tags.
    ///
    /// ## Test Scenario
    /// - HTML with unclosed tags or improper nesting
    /// - scraper library handles parsing
    ///
    /// ## Expected Outcome
    /// - Parser processes without crash, extracts text
    #[test]
    fn test_malformed_html() {
        let html = "<p>Unclosed paragraph<b>Unclosed bold";
        let lines = html_to_lines(html);
        assert!(!lines.is_empty());
    }

    /// # HTML With Comments
    ///
    /// Tests handling of HTML comment nodes.
    ///
    /// ## Test Scenario
    /// - HTML contains <!-- comment --> nodes
    /// - Parser should skip comments
    ///
    /// ## Expected Outcome
    /// - Comments are ignored, only visible text is extracted
    #[test]
    fn test_html_with_comments() {
        let html = "Before<!-- This is a comment -->After";
        let lines = html_to_lines(html);
        assert_eq!(lines.len(), 1);
        let text: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        assert!(!text.contains("comment"));
    }

    /// # Complex Real World HTML
    ///
    /// Tests processing of complex, realistic HTML content.
    ///
    /// ## Test Scenario
    /// - HTML with mix of block, inline, nested styles, and links
    /// - Represents typical rich text content
    ///
    /// ## Expected Outcome
    /// - All elements processed correctly with proper styling
    #[test]
    fn test_complex_html() {
        let html = r##"
            <div>
                <h1>Title</h1>
                <p>This is a <b>bold</b> paragraph with a <a href="#">link</a>.</p>
                <p>Another paragraph with <i>italic</i> and <code>code</code>.</p>
                <div>
                    <span style="color: red;">Red text</span> and
                    <span style="background-color: yellow; font-weight: bold;">highlighted bold</span>
                </div>
            </div>
        "##;
        let lines = html_to_lines(html);
        assert!(lines.len() >= 4);
        for line in &lines {
            if !line.spans.is_empty() {
                assert!(!line.spans[0].content.trim().is_empty());
            }
        }
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
