//! Dioxus WebSys
//!
//! ## Overview
//! ------------
//! This crate implements a renderer of the Dioxus Virtual DOM for the web browser using WebSys. This web render for
//! Dioxus is one of the more advanced renderers, supporting:
//! - idle work
//! - animations
//! - jank-free rendering
//! - noderefs
//! - controlled components
//! - re-hydration
//! - and more.
//!
//! The actual implementation is farily thin, with the heavy lifting happening inside the Dioxus Core crate.
//!
//! To purview the examples, check of the root Dioxus crate - the examples in this crate are mostly meant to provide
//! validation of websys-specific features and not the general use of Dioxus.
//!
//! ## RequestAnimationFrame and RequestIdleCallback
//! ------------------------------------------------
//! React implements "jank free rendering" by deliberately not blocking the browser's main thread. For large diffs, long
//! running work, and integration with things like React-Three-Fiber, it's extremeley important to avoid blocking the
//! main thread.
//!
//! React solves this problem by breaking up the rendering process into a "diff" phase and a "render" phase. In Dioxus,
//! the diff phase is non-blocking, using "yield_now" to allow the browser to process other events. When the diff phase
//! is  finally complete, the VirtualDOM will return a set of "Mutations" for this crate to apply.
//!
//! Here, we schedule the "diff" phase during the browser's idle period, achieved by calling RequestIdleCallback and then
//! setting a timeout from the that completes when the idleperiod is over. Then, we call requestAnimationFrame
//!
//!     From Google's guide on rAF and rIC:
//!     -----------------------------------
//!
//!     If the callback is fired at the end of the frame, it will be scheduled to go after the current frame has been committed,
//!     which means that style changes will have been applied, and, importantly, layout calculated. If we make DOM changes inside
//!      of the idle callback, those layout calculations will be invalidated. If there are any kind of layout reads in the next
//!      frame, e.g. getBoundingClientRect, clientWidth, etc, the browser will have to perform a Forced Synchronous Layout,
//!      which is a potential performance bottleneck.
//!
//!     Another reason not trigger DOM changes in the idle callback is that the time impact of changing the DOM is unpredictable,
//!     and as such we could easily go past the deadline the browser provided.
//!
//!     The best practice is to only make DOM changes inside of a requestAnimationFrame callback, since it is scheduled by the
//!     browser with that type of work in mind. That means that our code will need to use a document fragment, which can then
//!     be appended in the next requestAnimationFrame callback. If you are using a VDOM library, you would use requestIdleCallback
//!     to make changes, but you would apply the DOM patches in the next requestAnimationFrame callback, not the idle callback.
//!
//!     Essentially:
//!     ------------
//!     - Do the VDOM work during the idlecallback
//!     - Do DOM work in the next requestAnimationFrame callback

use std::rc::Rc;

pub use crate::cfg::WebConfig;
use crate::dom::load_document;
use cache::intern_cached_strings;
use dioxus::prelude::Properties;
use dioxus::virtual_dom::VirtualDom;
pub use dioxus_core as dioxus;
use dioxus_core::prelude::FC;
use futures_util::FutureExt;

mod cache;
mod cfg;
mod dom;
mod events;
mod nodeslab;
mod ric_raf;

/// Launch the VirtualDOM given a root component and a configuration.
///
/// This function expects the root component to not have root props. To launch the root component with root props, use
/// `launch_with_props` instead.
///
/// This method will block the thread with `spawn_local` from wasm_bindgen_futures.
///
/// If you need to run the VirtualDOM in its own thread, use `run_with_props` instead and await the future.
///
/// # Example
///
/// ```rust
/// fn main() {
///     dioxus_web::launch(App, |c| c);
/// }
///
/// static App: FC<()> = |cx, props| {
///     rsx!(cx, div {"hello world"})
/// }
/// ```
pub fn launch(root_component: FC<()>, configuration: impl FnOnce(WebConfig) -> WebConfig) {
    launch_with_props(root_component, (), configuration)
}

/// Launches the VirtualDOM from the specified component function and props.
///
/// This method will block the thread with `spawn_local`
///
/// # Example
///
/// ```rust
/// fn main() {
///     dioxus_web::launch_with_props(App, RootProps { name: String::from("joe") }, |c| c);
/// }
///
/// #[derive(ParitalEq, Props)]
/// struct RootProps {
///     name: String
/// }
///
/// static App: FC<RootProps> = |cx, props| {
///     rsx!(cx, div {"hello {props.name}"})
/// }
/// ```
pub fn launch_with_props<T, F>(root_component: FC<T>, root_properties: T, configuration_builder: F)
where
    T: Send + 'static,
    F: FnOnce(WebConfig) -> WebConfig,
{
    let config = configuration_builder(WebConfig::default());
    wasm_bindgen_futures::spawn_local(run_with_props(root_component, root_properties, config));
}

/// Runs the app as a future that can be scheduled around the main thread.
///
/// Polls futures internal to the VirtualDOM, hence the async nature of this function.
///
/// # Example
///
/// ```ignore
/// fn main() {
///     let app_fut = dioxus_web::run_with_props(App, RootProps { name: String::from("joe") }, |c| c);
///     wasm_bindgen_futures::spawn_local(app_fut);
/// }
/// ```
pub async fn run_with_props<T: 'static + Send>(root: FC<T>, root_props: T, cfg: WebConfig) {
    let mut dom = VirtualDom::new_with_props(root, root_props);

    intern_cached_strings();

    let should_hydrate = cfg.hydrate;

    let root_el = load_document().get_element_by_id(&cfg.rootname).unwrap();

    let tasks = dom.get_event_sender();

    let sender_callback = Rc::new(move |event| tasks.unbounded_send(event).unwrap());

    let mut websys_dom = dom::WebsysDom::new(root_el, cfg, sender_callback);

    let mut mutations = dom.rebuild();

    // hydrating is simply running the dom for a single render. If the page is already written, then the corresponding
    // ElementIds should already line up because the web_sys dom has already loaded elements with the DioxusID into memory
    if !should_hydrate {
        log::info!("Applying rebuild edits..., {:?}", mutations);
        websys_dom.process_edits(&mut mutations.edits);
    }

    let work_loop = ric_raf::RafLoop::new();

    loop {
        // if virtualdom has nothing, wait for it to have something before requesting idle time
        // if there is work then this future resolves immediately.
        dom.wait_for_work().await;

        // wait for the mainthread to schedule us in
        let mut deadline = work_loop.wait_for_idle_time().await;

        // run the virtualdom work phase until the frame deadline is reached
        let mutations = dom.run_with_deadline(|| (&mut deadline).now_or_never().is_some());

        // wait for the animation frame to fire so we can apply our changes
        work_loop.wait_for_raf().await;

        for mut edit in mutations {
            log::debug!("edits are {:#?}", edit);
            // actually apply our changes during the animation frame
            websys_dom.process_edits(&mut edit.edits);
        }
    }
}
