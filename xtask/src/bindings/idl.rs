//! The IDL: the public surface of `teigeodata-api`, read from its source with `syn`.
//!
//! Only the boundary types listed in that crate's documentation are accepted. Anything else is
//! an error that names the item and line, so an unsupported signature never reaches a
//! generator.

use serde::Serialize;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, FnArg, GenericArgument, Item, Lit, Pat, PathArguments, ReturnType};

/// Version of the IDL format itself.
pub(crate) const FORMAT: u32 = 4;

/// The extracted surface.
#[derive(Debug, Serialize)]
pub(crate) struct Idl {
    pub(crate) format: u32,
    pub(crate) source: &'static str,
    pub(crate) error_codes: Vec<Code>,
    pub(crate) constants: Vec<Constant>,
    pub(crate) functions: Vec<Function>,
    /// Opaque types with methods (D-274): each crosses as a handle, freed once.
    pub(crate) handles: Vec<Handle>,
    /// Plain data passed by value, options mostly (D-276).
    pub(crate) records: Vec<Record>,
    /// Closed sets of named values, for record fields (D-277).
    pub(crate) enums: Vec<Enum>,
    /// Every distinct callback signature, for the targets that declare one type per signature.
    pub(crate) callbacks: Vec<Callback>,
}

/// One `ErrorCode` variant.
#[derive(Debug, Serialize)]
pub(crate) struct Code {
    pub(crate) name: String,
    pub(crate) value: i64,
    pub(crate) doc: Vec<String>,
}

/// One `pub const`.
#[derive(Debug, Serialize)]
pub(crate) struct Constant {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    pub(crate) value: i64,
    pub(crate) doc: Vec<String>,
}

/// One `pub fn`, free or in a handle's `impl`.
#[derive(Debug, Serialize)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) doc: Vec<String>,
    /// A method on `&self` (only inside a handle's `impl`); otherwise a free or associated
    /// function.
    pub(crate) receiver: bool,
    pub(crate) params: Vec<Param>,
    pub(crate) returns: Ty,
    /// The handle type returned, when `returns` is [`Ty::Handle`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) returns_handle: Option<String>,
    pub(crate) fallible: bool,
}

/// One parameter. `callback` is set when `ty` is [`Ty::Callback`], `handle` when it is
/// [`Ty::Handle`].
#[derive(Debug, Serialize)]
pub(crate) struct Param {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) callback: Option<Callback>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) handle: Option<String>,
    /// The record type, when `ty` is [`Ty::Record`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) record: Option<String>,
}

/// A `pub struct` with only `pub` fields and `#[derive(Default)]`: every field's default is its
/// zero value (D-276).
#[derive(Debug, Serialize)]
pub(crate) struct Record {
    pub(crate) name: String,
    pub(crate) snake: String,
    pub(crate) doc: Vec<String>,
    pub(crate) fields: Vec<Field>,
}

/// One field of a record: a scalar, an `Option` of one (`optional`), or an enum.
#[derive(Debug, Serialize)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    pub(crate) optional: bool,
    /// The enum type, when `ty` is [`Ty::Enum`].
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub(crate) enum_name: Option<String>,
    pub(crate) doc: Vec<String>,
}

/// A `pub enum` of unit variants with explicit values from 0, and `#[derive(Default)]` with
/// `#[default]` on the variant valued 0, so a zeroed record holds its default (D-277).
#[derive(Debug, Serialize)]
pub(crate) struct Enum {
    pub(crate) name: String,
    pub(crate) snake: String,
    pub(crate) doc: Vec<String>,
    pub(crate) variants: Vec<Variant>,
}

/// One variant of an [`Enum`]; `snake` is its name in `snake_case` (`lazy`).
#[derive(Debug, Serialize)]
pub(crate) struct Variant {
    pub(crate) name: String,
    pub(crate) snake: String,
    pub(crate) value: i64,
    pub(crate) doc: Vec<String>,
}

/// A `pub struct` with private fields and the `pub fn`s of its `impl` blocks.
#[derive(Debug, Serialize)]
pub(crate) struct Handle {
    pub(crate) name: String,
    /// The name in `snake_case`, for C symbols (`tg_engine_search`).
    pub(crate) snake: String,
    pub(crate) doc: Vec<String>,
    pub(crate) methods: Vec<Function>,
}

/// A callback's signature: scalar arguments, a `bool` result (`false` cancels). `id` names the
/// signature (`u32`, `u32_f64`), so targets can declare one type per signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Callback {
    pub(crate) id: String,
    pub(crate) args: Vec<Ty>,
    pub(crate) returns: Ty,
}

/// A boundary type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Ty {
    Void,
    Bool,
    I32,
    U32,
    I64,
    U64,
    F64,
    /// `&str` in.
    Str,
    /// `&[u8]` in.
    Bytes,
    /// A handle: `&Name` in, `Name` (or `Self`) out.
    Handle,
    /// `String` out.
    String,
    /// `Blob` out.
    Blob,
    /// `&mut dyn FnMut(..) -> bool` in.
    Callback,
    /// A record, `&Name` in.
    Record,
    /// An enum, as a record field.
    Enum,
}

impl Ty {
    fn scalar(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "i32" => Self::I32,
            "u32" => Self::U32,
            "i64" => Self::I64,
            "u64" => Self::U64,
            "f64" => Self::F64,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Bool => "bool",
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::F64 => "f64",
            Self::Str => "str",
            Self::Bytes => "bytes",
            Self::Handle => "handle",
            Self::String => "string",
            Self::Blob => "blob",
            Self::Callback => "callback",
            Self::Record => "record",
            Self::Enum => "enum",
        }
    }

    fn is_scalar(self) -> bool {
        matches!(
            self,
            Self::Bool | Self::I32 | Self::U32 | Self::I64 | Self::U64 | Self::F64
        )
    }
}

/// Reads `source` (the text of `crates/teigeodata-api/src/lib.rs`).
pub(crate) fn extract(source: &str) -> Result<Idl, Vec<String>> {
    let file = syn::parse_file(source).map_err(|e| vec![at(e.span(), &e.to_string())])?;
    let mut idl = Idl {
        format: FORMAT,
        source: "crates/teigeodata-api/src/lib.rs",
        error_codes: Vec::new(),
        constants: Vec::new(),
        functions: Vec::new(),
        handles: Vec::new(),
        records: Vec::new(),
        enums: Vec::new(),
        callbacks: Vec::new(),
    };
    let mut problems = Vec::new();
    // Enums first, so a record's field may name one.
    for item in &file.items {
        if let Item::Enum(e) = item
            && is_pub(&e.vis)
            && e.ident != "ErrorCode"
        {
            match enumeration(e) {
                Ok(e) => idl.enums.push(e),
                Err(p) => problems.push(p),
            }
        }
    }
    let enums: Vec<String> = idl.enums.iter().map(|e| e.name.clone()).collect();
    // Handle and record types first, so a signature anywhere may name any of them.
    for item in &file.items {
        if let Item::Struct(s) = item
            && is_pub(&s.vis)
            && s.ident != "Error"
        {
            let name = s.ident.to_string();
            let public = s.fields.iter().filter(|f| is_pub(&f.vis)).count();
            if !s.generics.params.is_empty() {
                problems.push(at(s.span(), &format!("`{name}`: no generics")));
            } else if public > 0 {
                match record(s, &enums) {
                    Ok(r) => idl.records.push(r),
                    Err(p) => problems.push(p),
                }
            } else {
                idl.handles.push(Handle {
                    snake: snake(&name),
                    name,
                    doc: docs(&s.attrs),
                    methods: Vec::new(),
                });
            }
        }
    }
    let names = Names {
        handles: idl.handles.iter().map(|h| h.name.clone()).collect(),
        records: idl.records.iter().map(|r| r.name.clone()).collect(),
    };
    for item in &file.items {
        let result = match item {
            Item::Fn(f) if is_pub(&f.vis) => {
                function(&f.sig, &f.attrs, None, &names).map(|f| idl.functions.push(f))
            }
            Item::Impl(i) if i.trait_.is_none() => methods(i, &names).map(|(owner, mut m)| {
                if let Some(h) = idl.handles.iter_mut().find(|h| h.name == owner) {
                    h.methods.append(&mut m);
                }
            }),
            Item::Const(c) if is_pub(&c.vis) => constant(c).map(|c| idl.constants.push(c)),
            Item::Enum(e) if e.ident == "ErrorCode" => {
                codes(e).map(|mut c| idl.error_codes.append(&mut c))
            }
            _ => Ok(()),
        };
        if let Err(p) = result {
            problems.push(p);
        }
    }
    let all = idl
        .functions
        .iter()
        .chain(idl.handles.iter().flat_map(|h| &h.methods));
    for cb in all
        .flat_map(|f| &f.params)
        .filter_map(|p| p.callback.as_ref())
    {
        if !idl.callbacks.contains(cb) {
            idl.callbacks.push(cb.clone());
        }
    }
    if idl.error_codes.is_empty() {
        problems.push("no `ErrorCode` enum".into());
    }
    if problems.is_empty() {
        Ok(idl)
    } else {
        Err(problems)
    }
}

/// The type names a signature may use.
struct Names {
    handles: Vec<String>,
    records: Vec<String>,
}

/// A record (D-276): `pub` named fields only, each a scalar or an `Option` of one, and
/// `#[derive(Default)]` so each defaults to its zero value.
fn record(s: &syn::ItemStruct, enums: &[String]) -> Result<Record, String> {
    let name = s.ident.to_string();
    if !derives_default(&s.attrs) {
        return Err(at(
            s.span(),
            &format!("`{name}`: a record derives `Default` (D-276)"),
        ));
    }
    let mut fields = Vec::new();
    for f in &s.fields {
        let Some(id) = f.ident.as_ref().filter(|_| is_pub(&f.vis)) else {
            return Err(at(
                f.span(),
                &format!("`{name}`: a record's fields are named and `pub`"),
            ));
        };
        let enum_name = ident(&f.ty)
            .filter(|(seg, n)| matches!(seg.arguments, PathArguments::None) && enums.contains(n))
            .map(|(_, n)| n);
        let (ty, optional) = if enum_name.is_some() {
            (Ty::Enum, false)
        } else {
            field_ty(&f.ty).ok_or_else(|| {
                at(
                    f.ty.span(),
                    &format!(
                        "`{name}.{id}`: a record field is a scalar, an `Option` of one, or an enum"
                    ),
                )
            })?
        };
        fields.push(Field {
            name: id.to_string(),
            ty,
            optional,
            enum_name,
            doc: docs(&f.attrs),
        });
    }
    Ok(Record {
        snake: snake(&name),
        name,
        doc: docs(&s.attrs),
        fields,
    })
}

fn derives_default(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("derive")
            && a.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| paths.iter().any(|p| p.is_ident("Default")))
    })
}

/// An enum (D-277): unit variants, each with an explicit value, the values 0, 1, 2 … in order
/// (so a new variant is appended, and a value never changes meaning), `#[default]` on 0.
fn enumeration(e: &syn::ItemEnum) -> Result<Enum, String> {
    let name = e.ident.to_string();
    if !e.generics.params.is_empty() || !derives_default(&e.attrs) {
        return Err(at(
            e.span(),
            &format!("`{name}`: an enum has no generics and derives `Default` (D-277)"),
        ));
    }
    let mut variants = Vec::new();
    for (i, v) in e.variants.iter().enumerate() {
        let value = v.discriminant.as_ref().and_then(|(_, d)| int(d));
        let is_default = v.attrs.iter().any(|a| a.path().is_ident("default"));
        let fine = matches!(v.fields, syn::Fields::Unit)
            && value == i64::try_from(i).ok()
            && is_default == (i == 0);
        if !fine {
            return Err(at(
                v.span(),
                &format!(
                    "`{name}::{}`: a unit variant valued {i}, `#[default]` only on the one \
                     valued 0 (D-277)",
                    v.ident
                ),
            ));
        }
        let vname = v.ident.to_string();
        variants.push(Variant {
            snake: snake(&vname),
            name: vname,
            value: value.unwrap_or_default(),
            doc: docs(&v.attrs),
        });
    }
    if variants.is_empty() {
        return Err(at(e.span(), &format!("`{name}`: an enum has variants")));
    }
    Ok(Enum {
        snake: snake(&name),
        name,
        doc: docs(&e.attrs),
        variants,
    })
}

/// A scalar, or `Option<scalar>` (`true`).
fn field_ty(ty: &syn::Type) -> Option<(Ty, bool)> {
    let (seg, name) = ident(ty)?;
    match &seg.arguments {
        PathArguments::None => Some((Ty::scalar(&name)?, false)),
        PathArguments::AngleBracketed(a) if name == "Option" && a.args.len() == 1 => {
            let GenericArgument::Type(inner) = a.args.first()? else {
                return None;
            };
            let (seg, inner) = ident(inner)?;
            matches!(seg.arguments, PathArguments::None).then_some(())?;
            Some((Ty::scalar(&inner)?, true))
        }
        _ => None,
    }
}

fn is_pub(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

fn at(span: proc_macro2::Span, msg: &str) -> String {
    let start = span.start();
    format!(
        "crates/teigeodata-api/src/lib.rs:{}:{}: {msg}",
        start.line,
        start.column + 1
    )
}

/// Doc comment lines, with intra-doc links reduced to their text.
fn docs(attrs: &[Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("doc")) {
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let Expr::Lit(lit) = &nv.value
            && let Lit::Str(s) = &lit.lit
        {
            let line = s.value();
            let line = line.strip_prefix(' ').unwrap_or(&line);
            out.push(unlink(line));
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// `[`name`]` and `[text](url)` become `` `name` `` and `text`.
fn unlink(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = after.find(']') else {
            out.push_str(after);
            return out;
        };
        let inner = after.get(1..close).unwrap_or_default();
        out.push_str(inner);
        rest = after.get(close + 1..).unwrap_or_default();
        if let Some(url) = rest.strip_prefix('(')
            && let Some(end) = url.find(')')
        {
            rest = url.get(end + 1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    out
}

/// `CamelCase` to `snake_case`.
fn snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// The `pub fn`s of `impl Handle { .. }`.
fn methods(i: &syn::ItemImpl, names: &Names) -> Result<(String, Vec<Function>), String> {
    let public = i
        .items
        .iter()
        .any(|it| matches!(it, syn::ImplItem::Fn(f) if is_pub(&f.vis)));
    let owner = ident(&i.self_ty).map(|(_, n)| n).unwrap_or_default();
    if !names.handles.contains(&owner) {
        // Private helpers on the error type are fine; public ones would not cross.
        return if public {
            Err(at(
                i.self_ty.span(),
                "`impl` of a type that is not a public handle",
            ))
        } else {
            Ok((owner, Vec::new()))
        };
    }
    if !i.generics.params.is_empty() {
        return Err(at(i.span(), &format!("`{owner}`: no generics")));
    }
    let mut out = Vec::new();
    for item in &i.items {
        if let syn::ImplItem::Fn(f) = item
            && is_pub(&f.vis)
        {
            out.push(function(&f.sig, &f.attrs, Some(&owner), names)?);
        }
    }
    Ok((owner, out))
}

fn function(
    sig: &syn::Signature,
    attrs: &[Attribute],
    owner: Option<&str>,
    names: &Names,
) -> Result<Function, String> {
    let name = sig.ident.to_string();
    if sig.asyncness.is_some()
        || !matches!(sig.safety, syn::Safety::Default)
        || !sig.generics.params.is_empty()
    {
        return Err(at(
            sig.span(),
            &format!("`{name}`: no async, unsafe or generics"),
        ));
    }
    let mut params = Vec::new();
    let mut receiver = false;
    for input in &sig.inputs {
        let pt = match input {
            FnArg::Receiver(r)
                if owner.is_some()
                    && matches!(r.kind, syn::ReceiverKind::Reference(_, _, None)) =>
            {
                receiver = true;
                continue;
            }
            FnArg::Receiver(_) => {
                return Err(at(
                    input.span(),
                    &format!("`{name}`: a method takes `&self` only (D-274)"),
                ));
            }
            FnArg::Typed(pt) => pt,
        };
        let Pat::Ident(id) = &*pt.pat else {
            return Err(at(pt.span(), &format!("`{name}`: name every parameter")));
        };
        let (ty, callback, handle, record) = param_ty(&pt.ty, names).ok_or_else(|| {
            at(
                pt.ty.span(),
                &format!("`{name}`: parameter type is not a boundary type"),
            )
        })?;
        params.push(Param {
            name: id.ident.to_string(),
            ty,
            callback,
            handle,
            record,
        });
    }
    if params.iter().filter(|p| p.ty == Ty::Callback).count() > 1 {
        return Err(at(sig.span(), &format!("`{name}`: at most one callback")));
    }
    let (returns, returns_handle, fallible) = match &sig.output {
        ReturnType::Default => (Ty::Void, None, false),
        ReturnType::Type(_, ty) => result_ty(ty, owner, &names.handles).ok_or_else(|| {
            at(
                ty.span(),
                &format!("`{name}`: return type is not a boundary type"),
            )
        })?,
    };
    Ok(Function {
        name,
        doc: docs(attrs),
        receiver,
        params,
        returns,
        returns_handle,
        fallible,
    })
}

/// The last path segment's name, if the type is a plain path.
fn ident(ty: &syn::Type) -> Option<(&syn::PathSegment, String)> {
    let syn::Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    Some((seg, seg.ident.to_string()))
}

type ParamTy = (Ty, Option<Callback>, Option<String>, Option<String>);

fn param_ty(ty: &syn::Type, names: &Names) -> Option<ParamTy> {
    if let syn::Type::Reference(r) = ty {
        return match (&*r.elem, r.mutability.is_some()) {
            (syn::Type::Path(_), false) => {
                let (seg, name) = ident(&r.elem)?;
                matches!(seg.arguments, PathArguments::None).then_some(())?;
                if name == "str" {
                    Some((Ty::Str, None, None, None))
                } else if names.records.contains(&name) {
                    Some((Ty::Record, None, None, Some(name)))
                } else {
                    names
                        .handles
                        .contains(&name)
                        .then_some((Ty::Handle, None, Some(name), None))
                }
            }
            (syn::Type::Slice(s), false) if ident(&s.elem)?.1 == "u8" => {
                Some((Ty::Bytes, None, None, None))
            }
            (syn::Type::TraitObject(t), true) => {
                Some((Ty::Callback, Some(callback(t)?), None, None))
            }
            _ => None,
        };
    }
    let (seg, name) = ident(ty)?;
    matches!(seg.arguments, PathArguments::None).then_some(())?;
    Some((Ty::scalar(&name)?, None, None, None))
}

/// `dyn FnMut(scalars..) -> bool`, nothing else.
fn callback(t: &syn::TypeTraitObject) -> Option<Callback> {
    let mut bounds = t.bounds.iter();
    let (Some(syn::TypeParamBound::Trait(b)), None) = (bounds.next(), bounds.next()) else {
        return None;
    };
    let seg = b.path.segments.last()?;
    (seg.ident == "FnMut").then_some(())?;
    let PathArguments::Parenthesized(sig) = &seg.arguments else {
        return None;
    };
    let args = sig
        .inputs
        .iter()
        .map(|a| Ty::scalar(&ident(&a.ty)?.1))
        .collect::<Option<Vec<_>>>()?;
    let ReturnType::Type(_, ret) = &sig.output else {
        return None;
    };
    (ident(ret)?.1 == "bool").then_some(())?;
    let id = args.iter().map(|t| t.name()).collect::<Vec<_>>().join("_");
    Some(Callback {
        id: if id.is_empty() { "void".into() } else { id },
        args,
        returns: Ty::Bool,
    })
}

fn out_ty(ty: &syn::Type, owner: Option<&str>, handles: &[String]) -> Option<(Ty, Option<String>)> {
    if let syn::Type::Tuple(t) = ty {
        return t.elems.is_empty().then_some((Ty::Void, None));
    }
    let (seg, name) = ident(ty)?;
    matches!(seg.arguments, PathArguments::None).then_some(())?;
    let name = match (name.as_str(), owner) {
        ("Self", Some(o)) => o.to_owned(),
        _ => name,
    };
    if handles.contains(&name) {
        return Some((Ty::Handle, Some(name)));
    }
    match name.as_str() {
        "String" => Some((Ty::String, None)),
        "Blob" => Some((Ty::Blob, None)),
        other => Some((Ty::scalar(other)?, None)),
    }
}

fn result_ty(
    ty: &syn::Type,
    owner: Option<&str>,
    handles: &[String],
) -> Option<(Ty, Option<String>, bool)> {
    let (seg, name) = ident(ty)?;
    if name == "Result" {
        let PathArguments::AngleBracketed(args) = &seg.arguments else {
            return None;
        };
        let mut args = args.args.iter();
        let (Some(GenericArgument::Type(inner)), None) = (args.next(), args.next()) else {
            return None;
        };
        let (t, h) = out_ty(inner, owner, handles)?;
        return Some((t, h, true));
    }
    let (t, h) = out_ty(ty, owner, handles)?;
    Some((t, h, false))
}

fn int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Lit(l) => match &l.lit {
            Lit::Int(i) => i.base10_parse().ok(),
            _ => None,
        },
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => int(&u.expr)?.checked_neg(),
        _ => None,
    }
}

fn constant(c: &syn::ItemConst) -> Result<Constant, String> {
    let name = c.ident.to_string();
    let ty = ident(&c.ty)
        .and_then(|(_, n)| Ty::scalar(&n))
        .filter(|t| t.is_scalar() && *t != Ty::Bool && *t != Ty::F64)
        .ok_or_else(|| at(c.ty.span(), &format!("`{name}`: constants are integers")))?;
    let value =
        int(&c.expr).ok_or_else(|| at(c.expr.span(), &format!("`{name}`: use a literal")))?;
    Ok(Constant {
        name,
        ty,
        value,
        doc: docs(&c.attrs),
    })
}

fn codes(e: &syn::ItemEnum) -> Result<Vec<Code>, String> {
    e.variants
        .iter()
        .map(|v| {
            let value = v.discriminant.as_ref().and_then(|(_, d)| int(d));
            let value = value
                .filter(|&n| n > 0 && n < 1000)
                .ok_or_else(|| at(v.span(), "every ErrorCode needs a value in 1..1000"))?;
            Ok(Code {
                name: v.ident.to_string(),
                value,
                doc: docs(&v.attrs),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Ty, extract, unlink};

    const API: &str = r"
/// Codes.
#[repr(i32)]
pub enum ErrorCode {
    /// Bad.
    InvalidArgument = 1,
}
/// The cap.
pub const MAX: u32 = 1_000;
/// Folds [`text`]; see [the note](https://x).
///
pub fn fold(text: &str) -> String { String::new() }
pub fn run(n: u32, on: &mut dyn FnMut(u32) -> bool) -> Result<u32> { Ok(n) }
pub fn noop() {}
fn private(x: Vec<u8>) {}
/// Options.
#[derive(Clone, Default)]
pub struct Opts {
    /// Near here.
    pub lat: Option<f64>,
    pub strict: bool,
    pub mode: Mode,
}
/// How.
#[derive(Clone, Copy, Default)]
pub enum Mode {
    /// Slow.
    #[default]
    Slow = 0,
    FastPath = 1,
}
/// An engine.
pub struct Engine { inner: u32 }
impl Engine {
    /// Opens one.
    pub fn open(bytes: &[u8]) -> Result<Self> { todo!() }
    pub fn search(&self, query: &str, other: &Engine) -> Blob { todo!() }
    pub fn search_with(&self, query: &str, options: &Opts) -> Blob { todo!() }
    fn hidden(&self) {}
}
";

    #[test]
    fn extracts_the_surface() {
        let idl = extract(API).unwrap();
        let names: Vec<_> = idl.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["fold", "run", "noop"]);
        let fold = &idl.functions[0];
        assert_eq!(fold.doc, ["Folds `text`; see the note."]);
        assert_eq!(
            (fold.params[0].ty, fold.returns, fold.fallible),
            (Ty::Str, Ty::String, false)
        );
        let run = &idl.functions[1];
        assert_eq!((run.returns, run.fallible), (Ty::U32, true));
        let cb = run.params[1].callback.as_ref().unwrap();
        assert_eq!(
            (cb.args.as_slice(), cb.returns),
            ([Ty::U32].as_slice(), Ty::Bool)
        );
        assert_eq!(idl.functions[2].returns, Ty::Void);
        assert_eq!(idl.constants[0].value, 1000);
        assert_eq!(idl.error_codes[0].value, 1);
        assert_eq!(idl.callbacks.len(), 1);
        assert_eq!(idl.callbacks[0].id, "u32");
        let h = &idl.handles[0];
        assert_eq!(
            (h.name.as_str(), h.snake.as_str(), h.methods.len()),
            ("Engine", "engine", 3)
        );
        let open = &h.methods[0];
        assert_eq!(
            (open.receiver, open.params[0].ty, open.returns),
            (false, Ty::Bytes, Ty::Handle)
        );
        assert_eq!(
            (open.returns_handle.as_deref(), open.fallible),
            (Some("Engine"), true)
        );
        let search = &h.methods[1];
        assert!(search.receiver);
        assert_eq!(search.params[1].handle.as_deref(), Some("Engine"));
        assert!(!idl.functions[0].receiver);
        let with = &h.methods[2].params[1];
        assert_eq!(
            (with.ty, with.record.as_deref()),
            (Ty::Record, Some("Opts"))
        );
        let r = &idl.records[0];
        assert_eq!((r.name.as_str(), r.snake.as_str()), ("Opts", "opts"));
        let f: Vec<_> = r
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.ty, f.optional))
            .collect();
        assert_eq!(
            f,
            [
                ("lat", Ty::F64, true),
                ("strict", Ty::Bool, false),
                ("mode", Ty::Enum, false)
            ]
        );
        assert_eq!(r.fields[0].doc, ["Near here."]);
        assert_eq!(r.fields[2].enum_name.as_deref(), Some("Mode"));
        let mode = &idl.enums[0];
        let variants: Vec<_> = mode
            .variants
            .iter()
            .map(|v| (v.name.as_str(), v.snake.as_str(), v.value))
            .collect();
        assert_eq!(
            variants,
            [("Slow", "slow", 0), ("FastPath", "fast_path", 1)]
        );
        assert_eq!(
            (mode.doc.as_slice(), mode.snake.as_str()),
            (["How.".to_owned()].as_slice(), "mode")
        );
    }

    #[test]
    fn rejects_what_cannot_cross() {
        for bad in [
            "pub fn f(x: Vec<u8>) {}",
            "pub fn f(x: &mut str) {}",
            "pub fn f() -> Option<u32> { None }",
            "pub fn f(x: f32) {}",
            "pub fn f<T>(x: u32) {}",
            "pub fn f(a: &mut dyn FnMut(u32) -> bool, b: &mut dyn FnMut(u32) -> bool) {}",
            "pub fn f(a: &mut dyn FnMut(&str) -> bool) {}",
            "pub fn f(a: &mut dyn FnMut(u32)) {}",
            "pub struct S(pub u32);",
            "pub struct S<T> { t: T }",
            "pub struct S { pub x: u32 }",
            "impl Unknown { pub fn f() {} }",
            "pub struct S { x: u32 } impl S { pub fn f(&mut self) {} }",
            "pub struct S { x: u32 } impl S { pub fn f(self) {} }",
            "pub struct S { x: u32 } impl S { pub fn f(x: &mut [u8]) {} }",
            "pub fn f(x: &Unknown) {}",
            "pub const X: f64 = 1.0;",
            "#[derive(Default)] pub struct S { pub x: String }",
            "#[derive(Default)] pub struct S { pub x: u32, y: u32 }",
            "#[derive(Default)] pub struct S { pub x: Option<Vec<u8>> }",
            "#[derive(Default)] pub struct S { pub x: Option<f32> }",
            "#[derive(Default)] pub struct S { pub x: Option<Mode> }",
            "pub enum E { #[default] A = 0 }",
            "#[derive(Default)] pub enum E { A = 0 }",
            "#[derive(Default)] pub enum E { #[default] A = 1 }",
            "#[derive(Default)] pub enum E { #[default] A = 0, B = 2 }",
            "#[derive(Default)] pub enum E { #[default] A = 0, B }",
            "#[derive(Default)] pub enum E { #[default] A = 0, B(u32) = 1 }",
            "#[derive(Default)] pub enum E { A = 0, #[default] B = 1 }",
            "#[derive(Default)] pub enum E {}",
        ] {
            let src = format!("{API}\n{bad}");
            let err = extract(&src).unwrap_err();
            assert!(err[0].contains("lib.rs:"), "{bad}: {err:?}");
        }
    }

    #[test]
    fn unlinks() {
        assert_eq!(unlink("see [`A`] and [b](http://c) [x"), "see `A` and b [x");
    }
}
