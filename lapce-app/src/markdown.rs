use floem::text::{
    Attrs, AttrsList, FamilyOwned, LineHeightValue, Style, TextLayout, Weight,
};
use lapce_core::{language::LapceLanguage, syntax::Syntax};
use lapce_xi_rope::Rope;
use lsp_types::MarkedString;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag};
use smallvec::SmallVec;

use crate::config::{LapceConfig, color::LapceColor};

#[derive(Clone)]
pub enum MarkdownContent {
    Text(TextLayout),
    Image { url: String, title: String },
    Separator,
    /// A rendered Mermaid diagram (SVG string).
    MermaidDiagram { svg: String },
}

pub fn parse_markdown(
    text: &str,
    line_height: f64,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
    parse_markdown_sized(text, line_height, config, config.ui.font_size() as f32)
}

/// Like `parse_markdown` but with a custom font size (useful for compact panels).
pub fn parse_markdown_sized(
    text: &str,
    line_height: f64,
    config: &LapceConfig,
    font_size: f32,
) -> Vec<MarkdownContent> {
    let mut res = Vec::new();

    let mut current_text = String::new();
    let code_font_family: Vec<FamilyOwned> =
        FamilyOwned::parse_list(&config.editor.font_family).collect();

    let default_attrs = Attrs::new()
        .color(config.color(LapceColor::EDITOR_FOREGROUND))
        .font_size(font_size)
        .line_height(LineHeightValue::Normal(line_height as f32));
    let mut attr_list = AttrsList::new(default_attrs.clone());

    let mut builder_dirty = false;

    let mut pos = 0;

    let mut tag_stack: SmallVec<[(usize, Tag); 4]> = SmallVec::new();

    let parser = Parser::new_ext(
        text,
        Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES,
    );
    let mut last_text = CowStr::from("");
    // Whether we should add a newline on the next entry
    // This is used so that we don't emit newlines at the very end of the generation
    let mut add_newline = false;
    for event in parser {
        // Add the newline since we're going to be outputting more
        if add_newline {
            current_text.push('\n');
            builder_dirty = true;
            pos += 1;
            add_newline = false;
        }

        match event {
            Event::Start(tag) => {
                tag_stack.push((pos, tag));
            }
            Event::End(end_tag) => {
                if let Some((start_offset, tag)) = tag_stack.pop() {
                    if end_tag != tag.to_end() {
                        tracing::warn!("Mismatched markdown tag");
                        continue;
                    }

                    if let Some(attrs) = attribute_for_tag(
                        default_attrs.clone(),
                        &tag,
                        &code_font_family,
                        config,
                    ) {
                        attr_list
                            .add_span(start_offset..pos.max(start_offset), attrs);
                    }

                    if should_add_newline_after_tag(&tag) {
                        add_newline = true;
                    }

                    match &tag {
                        Tag::CodeBlock(kind) => {
                            // Check if this is a mermaid diagram
                            let is_mermaid = matches!(
                                kind,
                                CodeBlockKind::Fenced(lang) if lang.as_ref().trim().eq_ignore_ascii_case("mermaid")
                            );

                            if is_mermaid {
                                // Render mermaid diagram to SVG inline
                                // Use a dark theme so diagrams look good in the
                                // IDE's dark UI.
                                let mermaid_src = last_text.as_ref();
                                let opts = mermaid_rs_renderer::RenderOptions {
                                    theme: dark_mermaid_theme(),
                                    layout: mermaid_rs_renderer::LayoutConfig::default(),
                                };
                                match mermaid_rs_renderer::render_with_options(mermaid_src, opts) {
                                    Ok(svg_string) => {
                                        // Flush any pending text before the diagram
                                        if builder_dirty {
                                            // Remove the mermaid source text that was appended
                                            if current_text.len() >= mermaid_src.len() {
                                                let new_len = current_text.len() - mermaid_src.len();
                                                current_text.truncate(new_len);
                                                pos = current_text.len();
                                            }
                                            if !current_text.is_empty() {
                                                let mut text_layout = TextLayout::new();
                                                text_layout.set_text(&current_text, attr_list, None);
                                                res.push(MarkdownContent::Text(text_layout));
                                            }
                                            attr_list = AttrsList::new(default_attrs.clone());
                                            current_text.clear();
                                            pos = 0;
                                            builder_dirty = false;
                                        }
                                        res.push(MarkdownContent::MermaidDiagram { svg: svg_string });
                                    }
                                    Err(e) => {
                                        // Rendering failed — fall back to showing as code
                                        tracing::warn!("Mermaid render failed: {e}");
                                        highlight_as_code(
                                            &mut attr_list,
                                            default_attrs.clone().family(&code_font_family),
                                            None,
                                            &last_text,
                                            start_offset,
                                            config,
                                        );
                                        builder_dirty = true;
                                    }
                                }
                            } else {
                                let language =
                                    if let CodeBlockKind::Fenced(language) = kind {
                                        md_language_to_lapce_language(language)
                                    } else {
                                        None
                                    };

                                highlight_as_code(
                                    &mut attr_list,
                                    default_attrs.clone().family(&code_font_family),
                                    language,
                                    &last_text,
                                    start_offset,
                                    config,
                                );
                                builder_dirty = true;
                            }
                        }
                        Tag::Image {
                            link_type: _,
                            dest_url: dest,
                            title,
                            id: _,
                        } => {
                            // TODO: Are there any link types that would change how the
                            // image is rendered?

                            if builder_dirty {
                                let mut text_layout = TextLayout::new();
                                text_layout.set_text(&current_text, attr_list, None);
                                res.push(MarkdownContent::Text(text_layout));
                                attr_list = AttrsList::new(default_attrs.clone());
                                current_text.clear();
                                pos = 0;
                                builder_dirty = false;
                            }

                            res.push(MarkdownContent::Image {
                                url: dest.to_string(),
                                title: title.to_string(),
                            });
                        }
                        _ => {
                            // Presumably?
                            builder_dirty = true;
                        }
                    }
                } else {
                    tracing::warn!("Unbalanced markdown tag")
                }
            }
            Event::Text(text) => {
                if let Some((_, tag)) = tag_stack.last() {
                    if should_skip_text_in_tag(tag) {
                        continue;
                    }
                }
                current_text.push_str(&text);
                pos += text.len();
                last_text = text;
                builder_dirty = true;
            }
            Event::Code(text) => {
                attr_list.add_span(
                    pos..pos + text.len(),
                    default_attrs.clone().family(&code_font_family),
                );
                current_text.push_str(&text);
                pos += text.len();
                builder_dirty = true;
            }
            // TODO: Some minimal 'parsing' of html could be useful here, since some things use
            // basic html like `<code>text</code>`.
            Event::Html(text) => {
                attr_list.add_span(
                    pos..pos + text.len(),
                    default_attrs
                        .clone()
                        .family(&code_font_family)
                        .color(config.color(LapceColor::MARKDOWN_BLOCKQUOTE)),
                );
                current_text.push_str(&text);
                pos += text.len();
                builder_dirty = true;
            }
            Event::HardBreak => {
                current_text.push('\n');
                pos += 1;
                builder_dirty = true;
            }
            Event::SoftBreak => {
                current_text.push(' ');
                pos += 1;
                builder_dirty = true;
            }
            Event::Rule => {}
            Event::FootnoteReference(_text) => {}
            Event::TaskListMarker(_text) => {}
            Event::InlineHtml(_) => {} // TODO(panekj): Implement
            Event::InlineMath(_) => {} // TODO(panekj): Implement
            Event::DisplayMath(_) => {} // TODO(panekj): Implement
        }
    }

    if builder_dirty {
        let mut text_layout = TextLayout::new();
        text_layout.set_text(&current_text, attr_list, None);
        res.push(MarkdownContent::Text(text_layout));
    }

    res
}

fn attribute_for_tag<'a>(
    default_attrs: Attrs<'a>,
    tag: &Tag,
    code_font_family: &'a [FamilyOwned],
    config: &LapceConfig,
) -> Option<Attrs<'a>> {
    use pulldown_cmark::HeadingLevel;
    match tag {
        Tag::Heading {
            level,
            id: _,
            classes: _,
            attrs: _,
        } => {
            // The size calculations are based on the em values given at
            // https://drafts.csswg.org/css2/#html-stylesheet
            let font_scale = match level {
                HeadingLevel::H1 => 2.0,
                HeadingLevel::H2 => 1.5,
                HeadingLevel::H3 => 1.17,
                HeadingLevel::H4 => 1.0,
                HeadingLevel::H5 => 0.83,
                HeadingLevel::H6 => 0.75,
            };
            let font_size = font_scale * config.ui.font_size() as f64;
            Some(
                default_attrs
                    .font_size(font_size as f32)
                    .weight(Weight::BOLD),
            )
        }
        Tag::BlockQuote(_block_quote) => Some(
            default_attrs
                .style(Style::Italic)
                .color(config.color(LapceColor::MARKDOWN_BLOCKQUOTE)),
        ),
        Tag::CodeBlock(_) => Some(default_attrs.family(code_font_family)),
        Tag::Emphasis => Some(default_attrs.style(Style::Italic)),
        Tag::Strong => Some(default_attrs.weight(Weight::BOLD)),
        // TODO: Strikethrough support
        Tag::Link {
            link_type: _,
            dest_url: _,
            title: _,
            id: _,
        } => {
            // TODO: Link support
            Some(default_attrs.color(config.color(LapceColor::EDITOR_LINK)))
        }
        // All other tags are currently ignored
        _ => None,
    }
}

/// Decides whether newlines should be added after a specific markdown tag
fn should_add_newline_after_tag(tag: &Tag) -> bool {
    !matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. }
    )
}

/// Whether it should skip the text node after a specific tag  
/// For example, images are skipped because it emits their title as a separate text node.  
fn should_skip_text_in_tag(tag: &Tag) -> bool {
    matches!(tag, Tag::Image { .. })
}

fn md_language_to_lapce_language(lang: &str) -> Option<LapceLanguage> {
    // TODO: There are many other names commonly used that should be supported
    LapceLanguage::from_name(lang)
}

/// Highlight the text in a richtext builder like it was a markdown codeblock
pub fn highlight_as_code(
    attr_list: &mut AttrsList,
    default_attrs: Attrs,
    language: Option<LapceLanguage>,
    text: &str,
    start_offset: usize,
    config: &LapceConfig,
) {
    let syntax = language.map(Syntax::from_language);

    let styles = syntax
        .map(|mut syntax| {
            syntax.parse(0, Rope::from(text), None);
            syntax.styles
        })
        .unwrap_or(None);

    if let Some(styles) = styles {
        for (range, style) in styles.iter() {
            if let Some(color) = style
                .fg_color
                .as_ref()
                .and_then(|fg| config.style_color(fg))
            {
                attr_list.add_span(
                    start_offset + range.start..start_offset + range.end,
                    default_attrs.clone().color(color),
                );
            }
        }
    }
}

pub fn from_marked_string(
    text: MarkedString,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
    match text {
        MarkedString::String(text) => parse_markdown(&text, 1.8, config),
        // This is a short version of a code block
        MarkedString::LanguageString(code) => {
            // TODO: We could simply construct the MarkdownText directly
            // Simply construct the string as if it was written directly
            parse_markdown(
                &format!("```{}\n{}\n```", code.language, code.value),
                1.8,
                config,
            )
        }
    }
}

pub fn from_plaintext(
    text: &str,
    line_height: f64,
    config: &LapceConfig,
) -> Vec<MarkdownContent> {
    let mut text_layout = TextLayout::new();
    text_layout.set_text(
        text,
        AttrsList::new(
            Attrs::new()
                .font_size(config.ui.font_size() as f32)
                .line_height(LineHeightValue::Normal(line_height as f32)),
        ),
        None,
    );
    vec![MarkdownContent::Text(text_layout)]
}

/// A dark Mermaid theme that looks good on the IDE's dark background.
fn dark_mermaid_theme() -> mermaid_rs_renderer::Theme {
    mermaid_rs_renderer::Theme {
        font_family: "Inter, ui-sans-serif, system-ui, -apple-system, sans-serif"
            .to_string(),
        font_size: 14.0,
        // Node fill – dark slate
        primary_color: "#1E293B".to_string(),
        // Text on nodes – light
        primary_text_color: "#E2E8F0".to_string(),
        // Node border
        primary_border_color: "#475569".to_string(),
        // Edge / arrow colour
        line_color: "#94A3B8".to_string(),
        secondary_color: "#334155".to_string(),
        tertiary_color: "#1E293B".to_string(),
        edge_label_background: "none".to_string(),
        // Subgraph / cluster
        cluster_background: "#0F172A".to_string(),
        cluster_border: "#334155".to_string(),
        // SVG background – transparent so the container bg shows through
        background: "transparent".to_string(),
        // Sequence diagram
        sequence_actor_fill: "#1E293B".to_string(),
        sequence_actor_border: "#475569".to_string(),
        sequence_actor_line: "#64748B".to_string(),
        sequence_note_fill: "#422006".to_string(),
        sequence_note_border: "#92400E".to_string(),
        sequence_activation_fill: "#334155".to_string(),
        sequence_activation_border: "#475569".to_string(),
        text_color: "#CBD5E1".to_string(),
        // Git graph – reuse defaults
        git_colors: [
            "hsl(240, 100%, 46.2745098039%)".to_string(),
            "hsl(60, 100%, 43.5294117647%)".to_string(),
            "hsl(80, 100%, 46.2745098039%)".to_string(),
            "hsl(210, 100%, 46.2745098039%)".to_string(),
            "hsl(180, 100%, 46.2745098039%)".to_string(),
            "hsl(150, 100%, 46.2745098039%)".to_string(),
            "hsl(300, 100%, 46.2745098039%)".to_string(),
            "hsl(0, 100%, 46.2745098039%)".to_string(),
        ],
        git_inv_colors: [
            "hsl(60, 100%, 3.7254901961%)".to_string(),
            "rgb(0, 0, 160.5)".to_string(),
            "rgb(48.8333333334, 0, 146.5000000001)".to_string(),
            "rgb(146.5000000001, 73.2500000001, 0)".to_string(),
            "rgb(146.5000000001, 0, 0)".to_string(),
            "rgb(146.5000000001, 0, 73.2500000001)".to_string(),
            "rgb(0, 146.5000000001, 0)".to_string(),
            "rgb(0, 146.5000000001, 146.5000000001)".to_string(),
        ],
        git_branch_label_colors: [
            "#ffffff".to_string(),
            "black".to_string(),
            "black".to_string(),
            "#ffffff".to_string(),
            "black".to_string(),
            "black".to_string(),
            "black".to_string(),
            "black".to_string(),
        ],
        git_commit_label_color: "#CBD5E1".to_string(),
        git_commit_label_background: "#1E293B".to_string(),
        git_tag_label_color: "#CBD5E1".to_string(),
        git_tag_label_background: "#1E293B".to_string(),
        git_tag_label_border: "#475569".to_string(),
        // Pie chart – dark-friendly palette
        pie_colors: [
            "#3B82F6".to_string(),
            "#10B981".to_string(),
            "#F59E0B".to_string(),
            "#EF4444".to_string(),
            "#8B5CF6".to_string(),
            "#EC4899".to_string(),
            "#06B6D4".to_string(),
            "#F97316".to_string(),
            "#14B8A6".to_string(),
            "#6366F1".to_string(),
            "#A855F7".to_string(),
            "#84CC16".to_string(),
        ],
        pie_title_text_size: 25.0,
        pie_title_text_color: "#E2E8F0".to_string(),
        pie_section_text_size: 17.0,
        pie_section_text_color: "#E2E8F0".to_string(),
        pie_legend_text_size: 17.0,
        pie_legend_text_color: "#E2E8F0".to_string(),
        pie_stroke_color: "#0F172A".to_string(),
        pie_stroke_width: 1.6,
        pie_outer_stroke_width: 1.6,
        pie_outer_stroke_color: "#334155".to_string(),
        pie_opacity: 0.85,
    }
}
