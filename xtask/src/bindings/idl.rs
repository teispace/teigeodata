//! The IDL: the public surface of `teigeodata-api`, read from its source with `syn`.
//!
//! Only the boundary types listed in that crate's documentation are accepted. Anything else is
//! an error that names the item and line, so an unsupported signature never reaches a
//! generator.

use serde::Serialize;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, FnArg, GenericArgument, Item, Lit, Pat, PathArguments, ReturnType};

/// Version of the IDL format itself.
pub(crate) const FORMAT: u32 = 1;

/// The extracted surface.
#[derive(Debug, Serialize)]
pub(crate) struct Idl {
    pub(crate) format: u32,
    pub(crate) source: &'static str,
    pub(crate) error_codes: Vec<Code>,
    pub(crate) constants: Vec<Constant>,
    pub(crate) functions: Vec<Function>,
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

/// One `pub fn`.
#[derive(Debug, Serialize)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) doc: Vec<String>,
    pub(crate) params: Vec<Param>,
    pub(crate) returns: Ty,
    pub(crate) fallible: bool,
}

/// One parameter. `callback` is set when `ty` is [`Ty::Callback`].
#[derive(Debug, Serialize)]
pub(crate) struct Param {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) callback: Option<Callback>,
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
    /// `String` out.
    String,
    /// `Blob` out.
    Blob,
    /// `&mut dyn FnMut(..) -> bool` in.
    Callback,
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
            Self::String => "string",
            Self::Blob => "blob",
            Self::Callback => "callback",
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
        callbacks: Vec::new(),
    };
    let mut problems = Vec::new();
    for item in &file.items {
        let result = match item {
            Item::Fn(f) if is_pub(&f.vis) => function(f).map(|f| idl.functions.push(f)),
            Item::Const(c) if is_pub(&c.vis) => constant(c).map(|c| idl.constants.push(c)),
            Item::Enum(e) if e.ident == "ErrorCode" => {
                codes(e).map(|mut c| idl.error_codes.append(&mut c))
            }
            // The error type, the result alias and re-exports are fixed by the boundary.
            Item::Struct(s) if is_pub(&s.vis) && s.ident != "Error" => {
                Err(at(s.span(), "a public struct cannot cross the boundary"))
            }
            Item::Enum(e) if is_pub(&e.vis) => {
                Err(at(e.span(), "only `ErrorCode` may be a public enum"))
            }
            _ => Ok(()),
        };
        if let Err(p) = result {
            problems.push(p);
        }
    }
    for cb in idl
        .functions
        .iter()
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

fn function(f: &syn::ItemFn) -> Result<Function, String> {
    let sig = &f.sig;
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
    for input in &sig.inputs {
        let FnArg::Typed(pt) = input else {
            return Err(at(input.span(), &format!("`{name}`: free functions only")));
        };
        let Pat::Ident(id) = &*pt.pat else {
            return Err(at(pt.span(), &format!("`{name}`: name every parameter")));
        };
        let (ty, callback) = param_ty(&pt.ty).ok_or_else(|| {
            at(
                pt.ty.span(),
                &format!("`{name}`: parameter type is not a boundary type"),
            )
        })?;
        params.push(Param {
            name: id.ident.to_string(),
            ty,
            callback,
        });
    }
    if params.iter().filter(|p| p.ty == Ty::Callback).count() > 1 {
        return Err(at(sig.span(), &format!("`{name}`: at most one callback")));
    }
    let (returns, fallible) = match &sig.output {
        ReturnType::Default => (Ty::Void, false),
        ReturnType::Type(_, ty) => result_ty(ty).ok_or_else(|| {
            at(
                ty.span(),
                &format!("`{name}`: return type is not a boundary type"),
            )
        })?,
    };
    Ok(Function {
        name,
        doc: docs(&f.attrs),
        params,
        returns,
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

fn param_ty(ty: &syn::Type) -> Option<(Ty, Option<Callback>)> {
    if let syn::Type::Reference(r) = ty {
        return match (&*r.elem, r.mutability.is_some()) {
            (syn::Type::Path(_), false) if ident(&r.elem)?.1 == "str" => Some((Ty::Str, None)),
            (syn::Type::TraitObject(t), true) => Some((Ty::Callback, Some(callback(t)?))),
            _ => None,
        };
    }
    let (seg, name) = ident(ty)?;
    matches!(seg.arguments, PathArguments::None).then_some(())?;
    Some((Ty::scalar(&name)?, None))
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

fn out_ty(ty: &syn::Type) -> Option<Ty> {
    if let syn::Type::Tuple(t) = ty {
        return t.elems.is_empty().then_some(Ty::Void);
    }
    let (seg, name) = ident(ty)?;
    matches!(seg.arguments, PathArguments::None).then_some(())?;
    match name.as_str() {
        "String" => Some(Ty::String),
        "Blob" => Some(Ty::Blob),
        other => Ty::scalar(other),
    }
}

fn result_ty(ty: &syn::Type) -> Option<(Ty, bool)> {
    let (seg, name) = ident(ty)?;
    if name == "Result" {
        let PathArguments::AngleBracketed(args) = &seg.arguments else {
            return None;
        };
        let mut args = args.args.iter();
        let (Some(GenericArgument::Type(inner)), None) = (args.next(), args.next()) else {
            return None;
        };
        return Some((out_ty(inner)?, true));
    }
    Some((out_ty(ty)?, false))
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
            "pub struct S;",
            "pub const X: f64 = 1.0;",
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
