// A body answer's type cannot be separated from its verdict: there is no way
// to take the answer apart or keep its type while dropping the verdict.
use verter_session_query::flow::slice::FrameAnswer;

fn parts(answer: FrameAnswer) {
    let _ = answer.clone().into_parts();
    let _ = answer.into_ty();
}

fn main() {}
