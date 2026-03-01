//! Reactivity lint rules — detect common Vue reactivity pitfalls.

mod no_ref_as_operand;
mod no_setup_props_reactivity_loss;

pub use no_ref_as_operand::NoRefAsOperand;
pub use no_setup_props_reactivity_loss::NoSetupPropsReactivityLoss;
