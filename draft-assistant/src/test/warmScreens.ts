// The screens App renders behind React.lazy, imported for their side effect of
// landing in the module registry.
//
// `lazy` races a dynamic import against whatever the test is waiting for. That
// import normally finishes within a frame, but it is CPU work like any other:
// when several vitest workers compete for cores — the repo's own pre-commit
// hook running while another agent runs the suite — it has been seen to outlast
// a five-second `waitFor`, and the test fails on a stopwatch rather than on
// behaviour. Evaluating the modules up front leaves `lazy` an already-resolved
// promise, so the Suspense boundary clears on the first tick at any load.
//
// Importing this module is the whole API; there is nothing to call.
import "../components/DraftScreen";
import "../components/SeasonScreen";
import "../components/Chat";
