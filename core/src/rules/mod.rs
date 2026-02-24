//! Rule engine parsers — expression language with variable binding and
//! three rule format parsers (arrow, table, block), plus the RETE
//! evaluation engine and Python bridge.

pub mod bridge;
pub mod engine;
pub mod expr;
pub mod format;

pub use expr::{Condition, Expression, Operator, PathPattern, PathSegment};
pub use format::{
    parse_arrow_rules, parse_block_rules, parse_rules_auto, parse_table_rules,
    Rule, RuleAction,
};
pub use engine::{ReteEngine, RuleMatch, EvalResult, EngineWarning};
pub use bridge::{
    DecoratorRegistry, DecoratorHandler, ExtractedPython, MarkdownExtraction,
    extract_python_from_markdown, generate_python_source, parse_inline_rules,
};
