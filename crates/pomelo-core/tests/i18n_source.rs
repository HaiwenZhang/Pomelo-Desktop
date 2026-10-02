//! A focused syntax gate for current user-message surfaces; extend with new surfaces.
use std::path::{Path, PathBuf};
use syn::{
    Expr,
    visit::{self, Visit},
};

#[derive(Default)]
struct MessageAudit {
    violations: Vec<String>,
}

impl MessageAudit {
    fn user_expression(&mut self, expression: &Expr, surface: &str) {
        match expression {
            Expr::Lit(literal) => {
                if let syn::Lit::Str(value) = &literal.lit {
                    // Product identity is not translated; all other literal copy is a violation.
                    if value.value() != "Pomelo" {
                        self.violations
                            .push(format!("{surface}: {:?}", value.value()));
                    }
                }
            }
            Expr::Macro(expression) if expression.mac.path.is_ident("format") => {
                self.violations
                    .push(format!("{surface}: format! bypasses the message schema"));
            }
            Expr::MethodCall(call) if call.method == "to_string" => {
                self.violations.push(format!(
                    "{surface}: to_string() bypasses structured diagnostics"
                ));
            }
            Expr::Call(call) if call.args.len() == 1 => {
                if let Expr::Path(path) = call.func.as_ref()
                    && path.path.is_ident("Some")
                {
                    self.user_expression(&call.args[0], surface);
                }
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for MessageAudit {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        // Test assertions and fixtures are not application copy.
        if item.attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr
                    .meta
                    .require_list()
                    .is_ok_and(|list| list.tokens.to_string() == "test")
        }) {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if ["child", "label", "tooltip", "title", "menu"]
            .iter()
            .any(|name| call.method == *name)
            && let Some(expression) = call.args.first()
        {
            self.user_expression(expression, &call.method.to_string());
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_field_value(&mut self, field: &'ast syn::FieldValue) {
        if let syn::Member::Named(name) = &field.member
            && (name == "prompt" || name == "title")
        {
            self.user_expression(&field.expr, &name.to_string());
        }
        visit::visit_field_value(self, field);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = call.func.as_ref() {
            let name = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if ["MenuItem::action", "Menu::new", "PopupMenuItem::new"].contains(&name.as_str())
                && let Some(expression) = call.args.first()
            {
                self.user_expression(expression, &name);
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, expression: &'ast syn::Macro) {
        if expression.path.is_ident("bail") || expression.path.is_ident("anyhow") {
            self.violations
                .push("unstructured bail!/anyhow! user error".into());
        }
        visit::visit_macro(self, expression);
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if attribute.path().is_ident("error")
            && let Ok(literal) = attribute.parse_args::<syn::LitStr>()
        {
            let value = literal.value();
            let code = value.split_whitespace().next().unwrap_or_default();
            if code.is_empty()
                || !code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            {
                self.violations.push(format!(
                    "error Display must use a stable technical code: {value:?}"
                ));
            }
        }
        visit::visit_attribute(self, attribute);
    }
}

fn rust_sources(directory: &Path, paths: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, paths);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            paths.push(path);
        }
    }
}

#[test]
fn application_surfaces_use_registered_messages_and_structured_diagnostics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut paths = Vec::new();
    for name in ["pomelo-core", "pomelo-import", "pomelo-render", "pomelo"] {
        rust_sources(&root.join(name).join("src"), &mut paths);
    }
    let mut violations = Vec::new();
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        let file = syn::parse_file(&source).unwrap();
        let mut audit = MessageAudit::default();
        audit.visit_file(&file);
        violations.extend(
            audit
                .violations
                .into_iter()
                .map(|violation| format!("{}: {violation}", path.display())),
        );
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn syntax_gate_detects_literal_labels_and_premature_error_stringification() {
    let file = syn::parse_file(r#"fn example() { view.child("Hardcoded message"); button.label("打开"); view.child(error.to_string()); bail!("bad file"); }"#).unwrap();
    let mut audit = MessageAudit::default();
    audit.visit_file(&file);
    assert_eq!(audit.violations.len(), 4);
}
