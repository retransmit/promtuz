use jni::JNIEnv;
use jni::objects::JObject;
use jni::sys::jobject;
use jni_macro::jni;

use crate::JC;
use crate::RUNTIME;
use crate::ndk::defer::KotlinDeferred;
use crate::quic::server::RELAY;
use crate::utils::AsJni;

#[jni(base = "com.promtuz.core", class = "API")]
pub extern "system" fn getPublicAddr(mut env: JNIEnv, _: JC) -> jobject {
    let (deferred, raw) = KotlinDeferred::new(&mut env);

    KotlinDeferred::cache(&mut env);

    RUNTIME.spawn(async move {
        let relay = RELAY.read().clone();

        let res = match relay {
            Some(r) => r.public_addr().await.ok(),
            None => None,
        };

        match res {
            Some(addr) => {
                // SAFETY: bind the GlobalRef to a named local so its drop
                // (which calls DeleteGlobalRef) is deferred to the end of the
                // match arm — *after* `complete_object` finishes borrowing
                // through `as_obj()`. Writing this as a single chained
                // expression (`addr.ip().as_jni().as_obj()`) lets the
                // GlobalRef temporary die at end-of-statement, leaving
                // `complete_object` with a dangling JObject and a JNI
                // use-after-free on the JVM side. Decision (vs. taking a
                // `GlobalRef` by value): keep the `&JObject` API because the
                // null branch below also passes a borrowed JObject and we
                // don't want to add an `Option` parameter purely for one
                // call site — the lifetime fix at the call site is enough.
                let g = addr.ip().as_jni();
                deferred.complete_object(g.as_obj());
            },
            None => deferred.complete_object(&JObject::null()),
        }
    });

    raw
}
