//! WAProto TypeScript type generator.
//!
//! Parses `waproto/src/whatsapp.rs` (prost-generated) and outputs TypeScript
//! type declarations matching the protobufjs format used by Baileys.
//!
//! Key features:
//! - Generates `interface IFoo` + `class Foo` for each prost Message struct
//! - Generates `enum Foo` for each prost Enumeration
//! - Handles prost Oneof enums as both flat fields (protobufjs compat) AND
//!   nested oneof object (prost serde compat) on the parent struct
//! - camelCase field names matching protobufjs/Baileys convention
//! - Proper nesting via `namespace` blocks matching proto package structure
//!
//! Usage: cargo run -p bridge-codegen --bin gen-proto-types > ../Baileys/WAProto/index.d.ts

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use syn::{Fields, GenericArgument, Item, PathArguments, Type, TypePath};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ProtoModule {
    messages: Vec<ProtoMessage>,
    enums: Vec<ProtoEnum>,
    oneofs: Vec<ProtoOneof>,
    submodules: BTreeMap<String, ProtoModule>,
}

impl Default for ProtoModule {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            enums: Vec::new(),
            oneofs: Vec::new(),
            submodules: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct ProtoMessage {
    /// Rust struct name (e.g. "InteractiveMessage")
    name: String,
    fields: Vec<ProtoField>,
}

#[derive(Debug, Clone)]
struct ProtoField {
    /// Rust field name in snake_case
    rust_name: String,
    /// TypeScript type string
    ts_type: String,
    /// Whether this is an Option<T> field
    optional: bool,
    /// If this is a oneof field, the path to the oneof enum
    /// e.g. "interactive_message::InteractiveMessage"
    oneof_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ProtoEnum {
    name: String,
    variants: Vec<(String, i32)>,
}

#[derive(Debug, Clone)]
struct ProtoOneof {
    name: String,
    variants: Vec<OneofVariant>,
}

#[derive(Debug, Clone)]
struct OneofVariant {
    /// Rust variant name (PascalCase)
    name: String,
    /// TypeScript type of the variant's inner value
    ts_type: String,
    /// The prost type annotation (message, string, bool, etc.)
    prost_type: String,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn find_waproto_source() -> PathBuf {
    // Try local path first (development)
    let local = PathBuf::from("../../whatsapp-rust/waproto/src/whatsapp.rs");
    if local.exists() {
        eprintln!("Using local path: {}", local.display());
        return local;
    }

    // Try Cargo git cache
    let lock_path = Path::new("../Cargo.lock");
    if let Ok(lock_content) = std::fs::read_to_string(lock_path) {
        for line in lock_content.lines() {
            if line.contains("whatsapp-rust") && line.contains('#') {
                if let Some(hash_start) = line.rfind('#') {
                    let full_hash = line[hash_start + 1..].trim_end_matches('"');
                    let short_hash = &full_hash[..7];
                    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| {
                        let home = std::env::var("HOME").expect("HOME not set");
                        format!("{home}/.cargo")
                    });
                    let checkouts = PathBuf::from(&cargo_home).join("git/checkouts");
                    if let Ok(entries) = std::fs::read_dir(&checkouts) {
                        for entry in entries.flatten() {
                            if entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with("whatsapp-rust-")
                            {
                                let candidate =
                                    entry.path().join(short_hash).join("waproto/src/whatsapp.rs");
                                if candidate.exists() {
                                    eprintln!("Using Cargo cache: {}", candidate.display());
                                    return candidate;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    panic!("Cannot find waproto/src/whatsapp.rs — run from codegen/ dir or ensure whatsapp-rust is in ../");
}

fn parse_waproto(path: &Path) -> ProtoModule {
    let content = std::fs::read_to_string(path).expect("Failed to read waproto source");
    let file = syn::parse_file(&content).expect("Failed to parse waproto source");

    let mut root = ProtoModule::default();
    parse_items(&file.items, &mut root);
    root
}

fn parse_items(items: &[Item], module: &mut ProtoModule) {
    for item in items {
        match item {
            Item::Struct(s) => {
                if !has_prost_message(&s.attrs) {
                    continue;
                }
                let name = s.ident.to_string();
                let fields = match &s.fields {
                    Fields::Named(named) => named
                        .named
                        .iter()
                        .filter(|f| matches!(f.vis, syn::Visibility::Public(_)))
                        .map(|f| {
                            let rust_name = f.ident.as_ref().unwrap().to_string().replace("r#", "");
                            let oneof_path = get_prost_oneof_path(&f.attrs);
                            let (ts_type, optional) = if oneof_path.is_some() {
                                // Oneof fields are Option<EnumType> — we'll resolve the type later
                                ("any".to_string(), true)
                            } else {
                                rust_type_to_ts(&f.ty, &f.attrs)
                            };
                            ProtoField {
                                rust_name,
                                ts_type,
                                optional,
                                oneof_path,
                            }
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                module.messages.push(ProtoMessage { name, fields });
            }
            Item::Enum(e) => {
                if has_prost_oneof(&e.attrs) {
                    let name = e.ident.to_string();
                    let variants = e
                        .variants
                        .iter()
                        .map(|v| {
                            let vname = v.ident.to_string();
                            let prost_type = get_prost_field_type(&v.attrs);
                            let ts_type = match &v.fields {
                                Fields::Unnamed(u) if u.unnamed.len() == 1 => {
                                    let (t, _) = rust_type_to_ts(&u.unnamed[0].ty, &[]);
                                    t
                                }
                                Fields::Unit => prost_type_to_ts(&prost_type),
                                _ => "any".to_string(),
                            };
                            OneofVariant {
                                name: vname,
                                ts_type,
                                prost_type,
                            }
                        })
                        .collect();
                    module.oneofs.push(ProtoOneof { name, variants });
                } else if has_prost_enumeration(&e.attrs) {
                    let name = e.ident.to_string();
                    let variants = e
                        .variants
                        .iter()
                        .filter_map(|v| {
                            let vname = v.ident.to_string();
                            if let Some((_, syn::Expr::Lit(lit))) = &v.discriminant {
                                if let syn::Lit::Int(i) = &lit.lit {
                                    return Some((vname, i.base10_parse::<i32>().unwrap_or(0)));
                                }
                            }
                            Some((vname, 0))
                        })
                        .collect();
                    module.enums.push(ProtoEnum { name, variants });
                }
            }
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    let mod_name = m.ident.to_string();
                    let sub = module
                        .submodules
                        .entry(mod_name)
                        .or_insert_with(ProtoModule::default);
                    parse_items(items, sub);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute helpers
// ---------------------------------------------------------------------------

fn has_prost_message(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        let Ok(ts) = a.parse_args::<proc_macro2::TokenStream>() else {
            return false;
        };
        ts.to_string().contains("prost :: Message")
    })
}

fn has_prost_oneof(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        let Ok(ts) = a.parse_args::<proc_macro2::TokenStream>() else {
            return false;
        };
        ts.to_string().contains("prost :: Oneof")
    })
}

fn has_prost_enumeration(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        let Ok(ts) = a.parse_args::<proc_macro2::TokenStream>() else {
            return false;
        };
        ts.to_string().contains("prost :: Enumeration")
    })
}

/// Extract the oneof path from `#[prost(oneof = "module::EnumName", tags = "...")]`
fn get_prost_oneof_path(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("prost") {
            let Ok(ts) = attr.parse_args::<proc_macro2::TokenStream>() else {
                continue;
            };
            let s = ts.to_string();
            if s.contains("oneof") {
                // Extract: oneof = "some_module::SomeEnum"
                if let Some(start) = s.find("oneof = \"") {
                    let rest = &s[start + 9..];
                    if let Some(end) = rest.find('"') {
                        return Some(rest[..end].to_string());
                    }
                }
            }
        }
    }
    None
}

/// Get the prost field type annotation (message, string, bool, bytes, etc.)
fn get_prost_field_type(attrs: &[syn::Attribute]) -> String {
    for attr in attrs {
        if attr.path().is_ident("prost") {
            let Ok(ts) = attr.parse_args::<proc_macro2::TokenStream>() else {
                continue;
            };
            let s = ts.to_string();
            if s.starts_with("message") {
                return "message".to_string();
            } else if s.starts_with("string") {
                return "string".to_string();
            } else if s.starts_with("bool") {
                return "bool".to_string();
            } else if s.starts_with("bytes") {
                return "bytes".to_string();
            } else if s.starts_with("uint32")
                || s.starts_with("int32")
                || s.starts_with("uint64")
                || s.starts_with("int64")
                || s.starts_with("float")
                || s.starts_with("double")
                || s.starts_with("sint32")
                || s.starts_with("sint64")
                || s.starts_with("fixed32")
                || s.starts_with("fixed64")
                || s.starts_with("sfixed32")
                || s.starts_with("sfixed64")
            {
                return "number".to_string();
            }
        }
    }
    "any".to_string()
}

fn prost_type_to_ts(prost_type: &str) -> String {
    match prost_type {
        "string" => "string".to_string(),
        "bool" => "boolean".to_string(),
        "bytes" => "Uint8Array".to_string(),
        "number" => "number".to_string(),
        _ => "any".to_string(),
    }
}

/// Check if field has `#[prost(enumeration = "...")]`
fn get_prost_enumeration(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("prost") {
            let Ok(ts) = attr.parse_args::<proc_macro2::TokenStream>() else {
                continue;
            };
            let s = ts.to_string();
            if let Some(start) = s.find("enumeration = \"") {
                let rest = &s[start + 15..];
                if let Some(end) = rest.find('"') {
                    return Some(rest[..end].to_string());
                }
            }
        }
    }
    None
}

/// Check if field has `#[prost(..., repeated, ...)]`
fn is_repeated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("prost") {
            return false;
        }
        let Ok(ts) = a.parse_args::<proc_macro2::TokenStream>() else {
            return false;
        };
        ts.to_string().contains("repeated")
    })
}

// ---------------------------------------------------------------------------
// Rust type → TypeScript type
// ---------------------------------------------------------------------------

fn rust_type_to_ts(ty: &Type, attrs: &[syn::Attribute]) -> (String, bool) {
    // Check for enumeration attribute — these are stored as i32 but map to a TS enum
    if let Some(enum_path) = get_prost_enumeration(attrs) {
        let ts = format!("proto.{}", rust_path_to_ts_path(&enum_path));
        if is_optional_type(ty) {
            return (format!("{ts}|null"), true);
        }
        return (ts, false);
    }

    // Check for repeated fields
    if is_repeated(attrs) {
        let inner = unwrap_vec(ty);
        let (inner_ts, _) = rust_type_to_ts_inner(inner.unwrap_or(ty));
        return (format!("{inner_ts}[]"), false);
    }

    rust_type_to_ts_inner(ty)
}

fn rust_type_to_ts_inner(ty: &Type) -> (String, bool) {
    match ty {
        Type::Path(TypePath { path, .. }) => {
            let last = path.segments.last().unwrap();
            let ident = last.ident.to_string();

            match ident.as_str() {
                "String" => ("string".to_string(), false),
                "bool" => ("boolean".to_string(), false),
                "u8" | "u16" | "u32" | "i8" | "i16" | "i32" | "f32" | "f64" => {
                    ("number".to_string(), false)
                }
                "u64" | "i64" => ("number|Long".to_string(), false),
                "Option" => {
                    if let Some(inner) = unwrap_generic(ty) {
                        let (inner_ts, _) = rust_type_to_ts_inner(inner);
                        (format!("({inner_ts}|null)"), true)
                    } else {
                        ("any".to_string(), true)
                    }
                }
                "Vec" => {
                    if let Some(inner) = unwrap_generic(ty) {
                        if is_u8_type(inner) {
                            return ("Uint8Array".to_string(), false);
                        }
                        let (inner_ts, _) = rust_type_to_ts_inner(inner);
                        (format!("{inner_ts}[]"), false)
                    } else {
                        ("any[]".to_string(), false)
                    }
                }
                "Box" => {
                    if let Some(inner) = unwrap_generic(ty) {
                        rust_type_to_ts_inner(inner)
                    } else {
                        ("any".to_string(), false)
                    }
                }
                "Bytes" => ("Uint8Array".to_string(), false),
                // Known prost/proto types — use last meaningful segment
                other => {
                    (format!("proto.I{other}"), false)
                }
            }
        }
        _ => ("any".to_string(), false),
    }
}

/// Resolve a Rust type path to a TypeScript qualified name.
/// Handles `super::` prefixes and module-qualified paths.
fn resolve_ts_name(ident: &str, path: &syn::Path) -> String {
    // Collect all path segments
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();

    // Filter out prost/alloc/core prefixes and "super"
    // Keep only meaningful segments
    let meaningful: Vec<&str> = segments
        .iter()
        .filter(|s| *s != "super" && *s != "prost" && *s != "alloc" && *s != "core" && *s != "option" && *s != "vec" && *s != "boxed")
        .map(|s| s.as_str())
        .collect();

    if meaningful.is_empty() {
        return format!("I{ident}");
    }

    // The last segment is the type name, preceding segments are module names
    // Convert module names to PascalCase namespaces
    let last = meaningful.last().unwrap();
    format!("I{last}")
}

/// Convert a prost module path like "history_sync::HistorySyncType" to
/// a TypeScript namespace path like "HistorySync.HistorySyncType".
fn rust_path_to_ts_path(path: &str) -> String {
    let parts: Vec<&str> = path.split("::").collect();
    parts
        .iter()
        .filter(|p| **p != "super")
        .map(|p| to_pascal(p))
        .collect::<Vec<_>>()
        .join(".")
}

fn unwrap_generic(ty: &Type) -> Option<&Type> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(seg) = path.segments.last() {
            if let PathArguments::AngleBracketed(args) = &seg.arguments {
                if let Some(GenericArgument::Type(inner)) = args.args.first() {
                    return Some(inner);
                }
            }
        }
    }
    None
}

fn unwrap_vec(ty: &Type) -> Option<&Type> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(seg) = path.segments.last() {
            if seg.ident == "Vec" {
                return unwrap_generic(ty);
            }
        }
    }
    None
}

fn is_optional_type(ty: &Type) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(seg) = path.segments.last() {
            return seg.ident == "Option";
        }
    }
    false
}

fn is_u8_type(ty: &Type) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(seg) = path.segments.last() {
            return seg.ident == "u8";
        }
    }
    false
}

// ---------------------------------------------------------------------------
// TypeScript output
// ---------------------------------------------------------------------------

fn to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut upper_next = false;
    for (i, c) in s.chars().enumerate() {
        if c == '_' {
            upper_next = true;
            continue;
        }
        if upper_next && i > 0 {
            result.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

fn to_pascal(s: &str) -> String {
    let camel = to_camel(s);
    let mut chars = camel.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn emit_module(module: &ProtoModule, indent: usize, output: &mut String) {
    let pad = "    ".repeat(indent);

    // Emit enums with SCREAMING_SNAKE_CASE variants (matching protobufjs convention)
    for e in &module.enums {
        output.push_str(&format!("\n{pad}enum {name} {{\n", name = e.name));
        for (vname, val) in &e.variants {
            let screaming = to_screaming_snake(vname);
            output.push_str(&format!("{pad}    {screaming} = {val},\n"));
        }
        output.push_str(&format!("{pad}}}\n"));
    }

    // Emit messages
    for msg in &module.messages {
        let iname = format!("I{}", msg.name);

        // Collect oneof info for this message
        let oneofs: Vec<(&ProtoField, Option<&ProtoOneof>)> = msg
            .fields
            .iter()
            .filter(|f| f.oneof_path.is_some())
            .map(|f| {
                let oneof_enum = f.oneof_path.as_ref().and_then(|path| {
                    // Find the oneof enum in submodules
                    // path is like "interactive_message::InteractiveMessage"
                    let parts: Vec<&str> = path.split("::").collect();
                    if parts.len() == 2 {
                        let mod_name = parts[0];
                        let enum_name = parts[1];
                        module
                            .submodules
                            .get(mod_name)
                            .and_then(|sub| sub.oneofs.iter().find(|o| o.name == enum_name))
                    } else {
                        module.oneofs.iter().find(|o| o.name == parts[0])
                    }
                });
                (f, oneof_enum)
            })
            .collect();

        // Interface
        output.push_str(&format!("\n{pad}interface {iname} {{\n"));
        for f in &msg.fields {
            if f.oneof_path.is_some() {
                // For oneof fields, emit:
                // 1. Flat variant fields (protobufjs compat — used by sendMessage)
                // 2. Nested oneof object (prost compat — used by relayMessage/encodeProto)
                if let Some((_, Some(oneof))) = oneofs.iter().find(|(of, _)| of.rust_name == f.rust_name) {
                    // Flat variants (protobufjs style)
                    for v in &oneof.variants {
                        let camel_name = to_camel(&snake_from_pascal(&v.name));
                        output.push_str(&format!(
                            "{pad}    {camel_name}?: ({ts_type}|null);\n",
                            ts_type = v.ts_type
                        ));
                    }
                    // Nested oneof object (prost style)
                    let camel_field = to_camel(&f.rust_name);
                    output.push_str(&format!("{pad}    /** Prost oneof field */\n"));
                    output.push_str(&format!("{pad}    {camel_field}?: {{\n"));
                    for v in &oneof.variants {
                        let camel_name = to_camel(&snake_from_pascal(&v.name));
                        output.push_str(&format!(
                            "{pad}        {camel_name}?: ({ts_type}|null);\n",
                            ts_type = v.ts_type
                        ));
                    }
                    output.push_str(&format!("{pad}    }} | null;\n"));
                }
                continue;
            }
            let camel_name = to_camel(&f.rust_name);
            // All proto fields are optional in interfaces (proto3 semantics)
            output.push_str(&format!(
                "{pad}    {camel_name}?: {ts_type};\n",
                ts_type = f.ts_type
            ));
        }
        output.push_str(&format!("{pad}}}\n"));

        // Class with static methods (protobufjs compat)
        output.push_str(&format!("\n{pad}class {name} implements {iname} {{\n", name = msg.name));
        output.push_str(&format!("{pad}    constructor(p?: {iname});\n"));
        // Repeat fields as public properties
        for f in &msg.fields {
            if f.oneof_path.is_some() {
                if let Some((_, Some(oneof))) = oneofs.iter().find(|(of, _)| of.rust_name == f.rust_name) {
                    for v in &oneof.variants {
                        let camel_name = to_camel(&snake_from_pascal(&v.name));
                        output.push_str(&format!(
                            "{pad}    public {camel_name}?: ({ts_type}|null);\n",
                            ts_type = v.ts_type
                        ));
                    }
                    // Oneof discriminator (string union of variant names)
                    let camel_field = to_camel(&f.rust_name);
                    let variant_names: Vec<String> = oneof
                        .variants
                        .iter()
                        .map(|v| format!("\"{}\"", to_camel(&snake_from_pascal(&v.name))))
                        .collect();
                    output.push_str(&format!(
                        "{pad}    public {camel_field}?: ({variants});\n",
                        variants = variant_names.join("|")
                    ));
                }
                continue;
            }
            let camel_name = to_camel(&f.rust_name);
            let opt = if f.optional { "?" } else { "" };
            output.push_str(&format!(
                "{pad}    public {camel_name}{opt}: {ts_type};\n",
                ts_type = f.ts_type
            ));
        }
        // Static methods
        output.push_str(&format!(
            "{pad}    public static create(properties?: {iname}): {name};\n",
            name = msg.name
        ));
        output.push_str(&format!(
            "{pad}    public static encode(m: {iname}, w?: $protobuf.Writer): $protobuf.Writer;\n"
        ));
        output.push_str(&format!(
            "{pad}    public static decode(r: ($protobuf.Reader|Uint8Array), l?: number): {name};\n",
            name = msg.name
        ));
        output.push_str(&format!(
            "{pad}    public static fromObject(d: {{ [k: string]: any }}): {name};\n",
            name = msg.name
        ));
        output.push_str(&format!(
            "{pad}    public static toObject(m: {name}, o?: $protobuf.IConversionOptions): {{ [k: string]: any }};\n",
            name = msg.name
        ));
        output.push_str(&format!("{pad}    public toJSON(): {{ [k: string]: any }};\n"));
        output.push_str(&format!("{pad}}}\n"));
    }

    // Emit submodule namespaces
    for (mod_name, sub) in &module.submodules {
        // Find the parent message this module belongs to (prost convention: mod name = snake_case of message name)
        let ns_name = to_pascal(mod_name);

        // Only emit namespace if it has content
        if sub.messages.is_empty() && sub.enums.is_empty() && sub.submodules.is_empty() && sub.oneofs.is_empty() {
            continue;
        }

        // Check if there's content beyond just oneofs
        let has_real_content = !sub.messages.is_empty() || !sub.enums.is_empty() || !sub.submodules.is_empty();
        if !has_real_content {
            continue;
        }

        output.push_str(&format!("\n{pad}namespace {ns_name} {{\n"));
        emit_module(sub, indent + 1, output);
        output.push_str(&format!("{pad}}}\n"));
    }
}

/// Convert PascalCase to SCREAMING_SNAKE_CASE (for protobufjs enum compat)
fn to_screaming_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            // Don't insert underscore between consecutive uppercase (e.g. "E2EE" → "E2EE")
            let prev = s.as_bytes()[i - 1];
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() {
                result.push('_');
            }
        }
        result.push(c.to_ascii_uppercase());
    }
    result
}

/// Convert PascalCase to snake_case
fn snake_from_pascal(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let ts_only = std::env::args().any(|a| a == "--ts-only");
    let source = find_waproto_source();
    let root = parse_waproto(&source);

    let mut ts = String::new();

    // TypeScript header stubs
    ts.push_str("type Long = number;\n");
    ts.push_str("declare namespace $protobuf {\n");
    ts.push_str("    interface Writer { finish(): Uint8Array; }\n");
    ts.push_str("    type Reader = Uint8Array;\n");
    ts.push_str("    interface IConversionOptions { [key: string]: any; }\n");
    ts.push_str("}\n");
    ts.push_str("\nexport namespace proto {\n");

    emit_module(&root, 1, &mut ts);

    ts.push_str("}\n");

    if ts_only {
        // Output raw TypeScript (for WAProto/index.d.ts)
        print!("{ts}");
    } else {
        // Output as Rust source file with typescript_custom_section
        println!("//! Auto-generated WAProto TypeScript type declarations.");
        println!("//! Generated by: bun run gen:proto-types");
        println!("//! DO NOT EDIT — re-run gen:proto-types to regenerate.");
        println!("//!");
        println!("//! Supports both protobufjs-style flat oneofs (sendMessage path)");
        println!("//! and prost-style nested oneofs (relayMessage/encodeProto path).");
        println!();
        println!("use wasm_bindgen::prelude::*;");
        println!();

        // Pick raw string delimiter that doesn't conflict with content
        let delim = if ts.contains("\"#") {{ "##" }} else {{ "#" }};

        println!("#[wasm_bindgen(typescript_custom_section)]");
        println!("const _TS_PROTO_TYPES: &str = r{delim}\"");
        print!("{ts}");
        println!("\"{delim};");
    }

    // Stats to stderr
    let count_messages = count_items(&root, |m| m.messages.len());
    let count_enums = count_items(&root, |m| m.enums.len());
    let count_oneofs = count_items(&root, |m| m.oneofs.len());
    eprintln!(
        "Generated: {count_messages} messages, {count_enums} enums, {count_oneofs} oneofs"
    );
}

fn count_items(module: &ProtoModule, counter: fn(&ProtoModule) -> usize) -> usize {
    let mut total = counter(module);
    for sub in module.submodules.values() {
        total += count_items(sub, counter);
    }
    total
}
