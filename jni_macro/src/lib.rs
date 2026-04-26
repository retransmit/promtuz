use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_macro_input, punctuated::Punctuated, Expr, ExprLit, ItemFn, Lit, Meta,
};

#[proc_macro_attribute]
pub fn jni(args: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let name = func.sig.ident.to_string();

    let attrs = &func.attrs;
    let vis = &func.vis;
    let sig = &func.sig;
    let block = &func.block;

    let args = match Punctuated::<Meta, syn::Token![,]>::parse_terminated.parse(args) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut base = String::new();
    let mut class = String::new();

    for arg in args {
        if let Meta::NameValue(nv) = arg {
            if nv.path.is_ident("base") {
                if let Expr::Lit(ExprLit { lit: Lit::Str(ref s), .. }) = nv.value {
                    base = s.value();
                }
            }
            if nv.path.is_ident("class") {
                if let Expr::Lit(ExprLit { lit: Lit::Str(ref s), .. }) = nv.value {
                    class = s.value();
                }
            }
        }
    }

    if base.is_empty() || class.is_empty() {
        return syn::Error::new_spanned(
            &func.sig.ident,
            "#[jni] requires both `base = \"a.b.c\"` and `class = \"Klass\"`",
        )
        .to_compile_error()
        .into();
    }

    let jni_name = format!(
        "Java_{}_{}_{}",
        jni_mangle(&base),
        jni_mangle(&class),
        jni_mangle(&name),
    );

    let panic_label = format!("Rust panic in JNI shim `{}`", name);

    // Wrap the user body in catch_unwind so a Rust panic cannot unwind across
    // the `extern "system"` boundary into the JVM (UB). On panic we log and
    // abort: aborting is strictly safer than UB and leaves a stack trace in
    // logcat. Returning a typed default would require inspecting `sig.output`
    // at every callsite — not worth the complexity until exception bridging
    // lands.
    quote! {
        #[export_name = #jni_name]
        #[allow(non_snake_case)]
        #(#attrs)*
        #vis #sig {
            match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(move || #block)) {
                Ok(__v) => __v,
                Err(_) => {
                    ::log::error!(#panic_label);
                    ::std::process::abort()
                }
            }
        }
    }
    .into()
}

/// JNI symbol-name encoding per the JNI spec (§Resolving Native Method Names).
///
/// `.`/`/` (separator) → `_`, `_` → `_1`, `;` → `_2`, `[` → `_3`,
/// non-alphanumeric ASCII or non-ASCII → `_0XXXX` (4 hex digits).
fn jni_mangle(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for ch in seg.chars() {
        match ch {
            '.' | '/' => out.push('_'),
            '_' => out.push_str("_1"),
            ';' => out.push_str("_2"),
            '[' => out.push_str("_3"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            c => out.push_str(&format!("_0{:04x}", c as u32)),
        }
    }
    out
}
