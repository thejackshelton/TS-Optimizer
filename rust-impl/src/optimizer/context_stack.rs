/// Manages hierarchical context tracking during AST walk.
///
/// The context stack tracks the nesting of components, JSX elements,
/// and other scopes to build correct display names for segments.

#[derive(Debug, Clone)]
pub struct ContextStack {
    stack: Vec<String>,
}

impl ContextStack {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub fn push(&mut self, ctx: &str) {
        self.stack.push(ctx.to_string());
    }

    pub fn pop(&mut self) -> Option<String> {
        self.stack.pop()
    }

    pub fn as_slice(&self) -> Vec<&str> {
        self.stack.iter().map(|s| s.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }
}

impl Default for ContextStack {
    fn default() -> Self {
        Self::new()
    }
}
