#[derive(Clone)]
enum LineVisitor<'a> {
    NewLine { str: &'a str, op: &'a Op },
    Inline { str: &'a str, op: &'a Op },
}

#[derive(PartialEq)]
enum ListType {
    Ordered,
    Bullet,
}

struct OpVistorCtx<'a> {
    ops: &'a [Op],
    insert_index: usize,
    current: Option<LineVisitor<'a>>,

    inline_buf: String,
}

impl<'a> OpVistorCtx<'a> {
    fn new(ops: &'a [Op]) -> Self {
        Self {
            ops,
            insert_index: 0,
            inline_buf: String::new(),
            current: None,
        }
    }

    pub fn has_inline(&self) -> bool {
        !self.inline_buf.is_empty()
    }

    pub fn append_inline(&mut self, str: &str) {
        self.inline_buf.push_str(str);
    }

    pub fn flush_inline<W: core::fmt::Write + ?Sized>(
        &mut self,
        dest: &mut W,
    ) -> askama::Result<()> {
        if !self.inline_buf.is_empty() {
            write!(dest, "{}", self.inline_buf.as_str())?;
            self.inline_buf.clear();
        }
        Ok(())
    }

    pub fn next(&mut self) -> Option<LineVisitor<'a>> {
        while true {
            match self.ops.last() {
                None => {
                    self.current = None;
                    break;
                }
                Some(op) => {
                    if op.is_text_insert() {
                        let str = op.value_as_string();
                        if self.insert_index < str.len() {
                            let is_inline = !str.contains('\n');
                            if is_inline {
                                self.insert_index = str.len();
                                self.current = Some(LineVisitor::Inline { str, op });
                                return self.current.clone();
                            }
                            let next = str[self.insert_index..].split('\n').next();
                            match next {
                                Some(r) => {
                                    self.insert_index += r.len() + 1;
                                    self.current = Some(LineVisitor::NewLine {
                                        str: r,
                                        op,
                                    });
                                }
                                None => {
                                    self.insert_index = str.len();
                                    self.current = Some(LineVisitor::Inline {
                                        str: str,
                                        op,
                                    });
                                }
                            }
                            return self.current.clone();
                        }
                    }
                    self.ops.split_last().map(|(last, rest)| {
                        self.ops = rest;
                        self.insert_index = 0;
                        last
                    });
                }
            }
        }
        return self.current.clone();
    }

    pub fn current(&mut self) -> Option<LineVisitor<'a>> {
        if self.current.is_none() {
            return self.next();
        }
        return self.current.clone();
    }
}

struct DeltaHTML<'a> {
    ops: &'a [Op],
}


impl FastWritable for DeltaHTML<'_> {
    fn write_into<W: core::fmt::Write + ?Sized>(
        &self,
        dest: &mut W,
        values: &dyn askama::Values,
    ) -> askama::Result<()> {
        pub fn get_list_tag(op: &Op) -> Option<ListType> {
            op.attributes().and_then(|attrs| {
                if let Some(Value::String(list_type)) = attrs.get("list") {
                    match list_type.as_str() {
                        "ordered" => {
                            return Some(ListType::Ordered);
                        }
                        "bullet" => {
                            return Some(ListType::Bullet);
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
        }

        pub fn vistor_list<'a, W: core::fmt::Write + ?Sized>(
            visitor: &mut OpVistorCtx<'a>,
            dest: &mut W
        ) -> askama::Result<bool> {
            let cur = visitor.current();
            if let Some(c) = cur {
                if let LineVisitor::NewLine { str, op } = c {
                    let mut list_tag = get_list_tag(&op);
                    if let Some(tag) = list_tag {
                        write!(dest, "<ul>")?;
                        while let Some(c) = visitor.next() {
                            match c {
                                LineVisitor::NewLine { str, op } => {
                                    let new_list_tag = get_list_tag(&op);
                                    if new_list_tag == None {
                                        break;
                                    }

                                    write!(dest, "<li>")?;
                                    visitor.flush_inline(dest)?;
                                    write!(dest, "{}", str)?;
                                    write!(dest, "</li>")?;
                                }
                                LineVisitor::Inline { str, op } => {
                                    inline_vistor::<W>(visitor, op, str)?;
                                }
                            }
                        }
                        write!(dest, "</ul>")?;
                        return Ok(true)
                    }
                }
            }
            return Ok(false);
        }

        pub fn vistor_header<'a, W: core::fmt::Write + ?Sized>(
            visitor: &mut OpVistorCtx<'a>,
            dest: &mut W
        ) -> askama::Result<bool> {  
            if let Some(c) = visitor.current(){
                if let LineVisitor::NewLine { str, op } = c {
                    if let Some(attrs) = op.attributes() {
                        if let Some(Value::Number(level)) = attrs.get("header") {
                            if let Some(l) = level.as_u64() {
                                let l = std::cmp::min(6, l);
                                write!(dest, "<h{}>", l)?;
                                visitor.flush_inline(dest)?;
                                write!(dest, "{}", str)?;
                                write!(dest, "</h{}>", l)?;
                                visitor.next();
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            Ok(false)
        }


        pub fn inline_vistor<'a, W: core::fmt::Write + ?Sized>(
            vistor: &mut OpVistorCtx<'a>,
            current: &'a Op,
            str: &'a str,
        ) -> askama::Result<()> {
            let mut is_bold = false;
            let mut is_italic = false;
            let mut is_underline = false;
            let mut is_strike = false;

            if let Some(attrs) = current.attributes() {
                if let Some(Value::Bool(bold)) = attrs.get("bold") {
                    is_bold = *bold;
                }
                if let Some(Value::Bool(italic)) = attrs.get("italic") {
                    is_italic = *italic;
                }
                if let Some(Value::Bool(underline)) = attrs.get("underline") {
                    is_underline = *underline;
                }
                if let Some(Value::Bool(strike)) = attrs.get("strike") {
                    is_strike = *strike;
                }
            }
            struct Tag {
                name: &'static str,
                enabled: bool,
            }
            let mut tags: [Tag; 4] = [
                Tag { name: "b", enabled: is_bold },
                Tag { name: "em", enabled: is_italic },
                Tag { name: "u", enabled: is_underline },
                Tag { name: "s", enabled: is_strike },
            ];

            for tag in tags.iter().rev() {
                if tag.enabled {
                    vistor.append_inline(&format!("<{}>", tag.name));
                }
            }
            vistor.append_inline(&str);
            for tag in tags.iter() {
                if tag.enabled {
                    vistor.append_inline(&format!("</{}>", tag.name));
                }
            }
            Ok(())
        }
        pub fn walk_visitor<'a, W: core::fmt::Write + ?Sized>(
            dest: &mut W,
            visitors: &mut OpVistorCtx<'a>,
        ) -> askama::Result<()> {
            while let Some(op) = visitors.current() {
                match op {
                    LineVisitor::NewLine { str, op } => {
                        if vistor_list(visitors, dest)? {
                            continue;
                        }
                        if vistor_header(visitors, dest)? {
                            continue;
                        }
                        write!(dest, "<p>")?;
                        visitors.flush_inline(dest)?;
                        write!(dest, "{}", str)?;
                        write!(dest, "</p>")?;
                    }
                    LineVisitor::Inline { str, op } => {
                        inline_vistor::<W>(visitors, op, str)?;
                    }
                }
                visitors.next();
            }

            if visitors.has_inline() {
                write!(dest, "<p>")?;
                visitors.flush_inline(dest)?;
                write!(dest, "</p>")?;
            }

            Ok(())
        }
        let mut stage = String::new();
        let mut visitors = OpVistorCtx::new(self.ops);
        walk_visitor(dest, &mut visitors)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quill::{
        attributes::AttributesMap,
        op::Op,
    };
    use askama::FastWritable;
    use serde_json::Value;


    fn render_delta_html(ops: Vec<Op>) -> String {
        let delta_html = DeltaHTML { ops: &ops };
        let mut output = String::new();
        delta_html.write_into(&mut output, &()).unwrap();
        output
    }

    #[test]
    fn test_simple_text_rendering() {
        let ops = vec![
            Op::insert("Hello, World!", None),
        ];
        
        let result = render_delta_html(ops);
        assert_eq!(result, "<p>Hello, World!</p>");
    }

    #[test]
    fn test_multiline_text_rendering() {
        let ops = vec![
            Op::insert("First line\nSecond line\nThird line", None),
        ];
        
        let result = render_delta_html(ops);
        assert_eq!(result, "<p>First line</p><p>Second line</p><p>Third line</p>");
    }

    #[test]
    fn test_bold_text_rendering() {
        let ops = vec![
            Op::insert("Bold text", Some(attributes!(
                "bold" => true
            ))),
        ];
        
        let result = render_delta_html(ops);
        assert_eq!(result, "<p><b>Bold text</b></p>");
    }

    #[test]
    fn test_italic_text_rendering() {
        let ops = vec![
            Op::insert("Italic text", Some(attributes!(
                "italic" => true
            ))),
        ];
        
        let result = render_delta_html(ops);
        assert_eq!(result, "<p><em>Italic text</em></p>");
    }

    #[test]
    fn test_underline_text_rendering() {
        let ops = vec![
            Op::insert("Underlined text", Some(attributes!(
                "underline" => true
            ))),
        ];
        
        let result = render_delta_html(ops);
        assert_eq!(result, "<p><u>Underlined text</u></p>");
    }

    //#[test]
    //fn test_strikethrough_text_rendering() {
    //    let attrs = create_attributes(vec![("strike", Value::Bool(true))]);
    //    let ops = vec![
    //        Op::insert("Strikethrough text", Some(attrs)),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<p><s>Strikethrough text</s></p>");
    //}

    //#[test]
    //fn test_multiple_formatting_attributes() {
    //    let attrs = create_attributes(vec![
    //        ("bold", Value::Bool(true)),
    //        ("italic", Value::Bool(true)),
    //        ("underline", Value::Bool(true)),
    //    ]);
    //    let ops = vec![
    //        Op::insert("Multi-formatted text", Some(attrs)),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<p><u><em><b>Multi-formatted text</b></em></u></p>");
    //}

    //#[test]
    //fn test_mixed_formatted_and_plain_text() {
    //    let bold_attrs = create_attributes(vec![("bold", Value::Bool(true))]);
    //    let ops = vec![
    //        Op::insert("Plain text ", None),
    //        Op::insert("bold text", Some(bold_attrs)),
    //        Op::insert(" more plain", None),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<p>Plain text <b>bold text</b> more plain</p>");
    //}

    //#[test]
    //fn test_bullet_list_rendering() {
    //    let list_attrs = create_attributes(vec![("list", Value::String("bullet".to_string()))]);
    //    let ops = vec![
    //        Op::insert("First item\n", Some(list_attrs.clone())),
    //        Op::insert("Second item\n", Some(list_attrs.clone())),
    //        Op::insert("Third item\n", Some(list_attrs)),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<ul><li>First item</li><li>Second item</li><li>Third item</li></ul>");
    //}

    //#[test]
    //fn test_ordered_list_rendering() {
    //    let list_attrs = create_attributes(vec![("list", Value::String("ordered".to_string()))]);
    //    let ops = vec![
    //        Op::insert("First item\n", Some(list_attrs.clone())),
    //        Op::insert("Second item\n", Some(list_attrs.clone())),
    //        Op::insert("Third item\n", Some(list_attrs)),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<ul><li>First item</li><li>Second item</li><li>Third item</li></ul>");
    //}

    //#[test]
    //fn test_list_with_formatted_text() {
    //    let list_attrs = create_attributes(vec![("list", Value::String("bullet".to_string()))]);
    //    let bold_list_attrs = create_attributes(vec![
    //        ("list", Value::String("bullet".to_string())),
    //        ("bold", Value::Bool(true)),
    //    ]);
    //    
    //    let ops = vec![
    //        Op::insert("Plain item\n", Some(list_attrs)),
    //        Op::insert("Bold item\n", Some(bold_list_attrs)),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<ul><li>Plain item</li><li><b>Bold item</b></li></ul>");
    //}

    //#[test]
    //fn test_mixed_content_with_list_and_paragraphs() {
    //    let list_attrs = create_attributes(vec![("list", Value::String("bullet".to_string()))]);
    //    let ops = vec![
    //        Op::insert("Regular paragraph\n", None),
    //        Op::insert("List item 1\n", Some(list_attrs.clone())),
    //        Op::insert("List item 2\n", Some(list_attrs)),
    //        Op::insert("Another paragraph", None),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<p>Regular paragraph</p><ul><li>List item 1</li><li>List item 2</li></ul><p>Another paragraph</p>");
    //}

    //#[test]
    //fn test_empty_delta() {
    //    let ops = vec![];
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "");
    //}

    //#[test]
    //fn test_single_newline() {
    //    let ops = vec![
    //        Op::insert("\n", None),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<p></p>");
    //}

    //#[test]
    //fn test_multiple_empty_lines() {
    //    let ops = vec![
    //        Op::insert("\n\n\n", None),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    assert_eq!(result, "<p></p><p></p><p></p>");
    //}

    //#[test]
    //fn test_complex_document_structure() {
    //    let heading_attrs = create_attributes(vec![("bold", Value::Bool(true))]);
    //    let list_attrs = create_attributes(vec![("list", Value::String("bullet".to_string()))]);
    //    let italic_attrs = create_attributes(vec![("italic", Value::Bool(true))]);
    //    
    //    let ops = vec![
    //        Op::insert("Document Title\n", Some(heading_attrs)),
    //        Op::insert("This is a regular paragraph with some ", None),
    //        Op::insert("italic text", Some(italic_attrs)),
    //        Op::insert(" in it.\n", None),
    //        Op::insert("First bullet point\n", Some(list_attrs.clone())),
    //        Op::insert("Second bullet point\n", Some(list_attrs)),
    //        Op::insert("Final paragraph.", None),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    let expected = "<p><b>Document Title</b></p><p>This is a regular paragraph with some <em>italic text</em> in it.</p><ul><li>First bullet point</li><li>Second bullet point</li></ul><p>Final paragraph.</p>";
    //    assert_eq!(result, expected);
    //}

    //#[test]
    //fn test_op_visitor_ctx_functionality() {
    //    let ops = vec![
    //        Op::insert("Hello\nWorld", None),
    //    ];
    //    
    //    let mut ctx = OpVistorCtx::new(&ops);
    //    
    //    // Test first visitor (should be NewLine with "Hello")
    //    if let Some(LineVisitor::NewLine { str, .. }) = ctx.next() {
    //        assert_eq!(str, "Hello");
    //    } else {
    //        panic!("Expected NewLine visitor with 'Hello'");
    //    }
    //    
    //    // Test inline buffer functionality
    //    ctx.append_inline("<b>");
    //    ctx.append_inline("test");
    //    ctx.append_inline("</b>");
    //    
    //    let mut output = String::new();
    //    ctx.flush_inline(&mut output).unwrap();
    //    assert_eq!(output, "<b>test</b>");
    //    
    //    // Buffer should be empty after flush
    //    let mut output2 = String::new();
    //    ctx.flush_inline(&mut output2).unwrap();
    //    assert_eq!(output2, "");
    //}

    //#[test]
    //fn test_list_type_detection() {
    //    // Test ordered list detection
    //    let ordered_attrs = create_attributes(vec![("list", Value::String("ordered".to_string()))]);
    //    let ordered_op = Op::insert("Item", Some(ordered_attrs));
    //    
    //    // Test bullet list detection
    //    let bullet_attrs = create_attributes(vec![("list", Value::String("bullet".to_string()))]);
    //    let bullet_op = Op::insert("Item", Some(bullet_attrs));
    //    
    //    // Test non-list item
    //    let plain_op = Op::insert("Item", None);
    //    
    //    // Since get_list_tag is a nested function, we'll test it through the rendering
    //    let ordered_ops = vec![Op::insert("Item\n", Some(create_attributes(vec![("list", Value::String("ordered".to_string()))])))];
    //    let ordered_result = render_delta_html(ordered_ops);
    //    assert!(ordered_result.contains("<ul>") && ordered_result.contains("<li>Item</li>"));
    //    
    //    let bullet_ops = vec![Op::insert("Item\n", Some(create_attributes(vec![("list", Value::String("bullet".to_string()))])))];
    //    let bullet_result = render_delta_html(bullet_ops);
    //    assert!(bullet_result.contains("<ul>") && bullet_result.contains("<li>Item</li>"));
    //}

    //#[test]
    //fn test_invalid_list_type() {
    //    let invalid_list_attrs = create_attributes(vec![("list", Value::String("invalid".to_string()))]);
    //    let ops = vec![
    //        Op::insert("Should be paragraph\n", Some(invalid_list_attrs)),
    //    ];
    //    
    //    let result = render_delta_html(ops);
    //    // Should render as paragraph since "invalid" is not a recognized list type
    //    assert_eq!(result, "<p>Should be paragraph</p>");
    //}
}

