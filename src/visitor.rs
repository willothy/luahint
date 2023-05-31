use full_moon::ast::{
    Call, Expression, FunctionArgs, FunctionCall, FunctionDeclaration, Suffix, TableConstructor,
    Value,
};
use full_moon::node::Node;
use full_moon::visitors::Visitor;
use tower_lsp::lsp_types::*;

use crate::scope::{ScopeManager, Var};

impl ScopeManager {
    pub fn extract_params(
        &self,
        expr: &Expression,
    ) -> Option<Vec<(String, full_moon::tokenizer::Position)>> {
        match expr {
            Expression::Value { value } => match value.as_ref() {
                Value::Function((_, f)) => Some(
                    f.parameters()
                        .iter()
                        .map(|p| {
                            (
                                p.to_string().trim().to_string(),
                                p.start_position().unwrap_or_default(),
                            )
                        })
                        .collect::<Vec<_>>(),
                ),
                Value::ParenthesesExpression(expr) => self.extract_params(expr),
                Value::Var(var) => match var {
                    // full_moon::ast::Var::Expression(e) => ,
                    full_moon::ast::Var::Name(t) => {
                        let name = t.to_string().trim().to_string();
                        self.find_var(&name).and_then(|var| match var {
                            Var::Local(val_id) => self
                                .get_current_scope()?
                                .value_arena
                                .get(val_id)
                                .and_then(|val| match val {
                                    Value::Function((_, f)) => Some(
                                        f.parameters()
                                            .iter()
                                            .map(|p| {
                                                (
                                                    p.to_string().trim().to_string(),
                                                    p.start_position().unwrap_or_default(),
                                                )
                                            })
                                            .collect::<Vec<_>>(),
                                    ),
                                    _ => None,
                                }),
                            Var::Reference(scope_id, var_id) => {
                                let (scope, val) = self.resolve_reference(scope_id, var_id)?;
                                self.get_scope(scope)?.value_arena.get(val).and_then(
                                    |val| match val {
                                        Value::Function((_, f)) => Some(
                                            f.parameters()
                                                .iter()
                                                .map(|p| {
                                                    (
                                                        p.to_string().trim().to_string(),
                                                        p.start_position().unwrap_or_default(),
                                                    )
                                                })
                                                .collect::<Vec<_>>(),
                                        ),
                                        _ => None,
                                    },
                                )
                            }
                        })
                    }
                    _ => return None,
                },
                _ => return None,
            },
            Expression::Parentheses { expression, .. } => self.extract_params(expression),
            // Expression::BinaryOperator { lhs, binop, rhs } => {}
            // Expression::UnaryOperator { unop, expression } => {}
            _ => None,
        }
    }
}

pub trait Fields {
    fn named_fields(&self) -> Vec<(String, &Expression)>;
    fn named_field(&self, name: String) -> Option<&Expression>;

    fn indexed_fields(&self) -> Vec<&Expression>;
    fn indexed_field(&self, index: usize) -> Option<&Expression>;

    fn expr_fields(&self) -> Vec<(&Expression, &Expression)>;
    fn expr_field(&self, index: &Expression) -> Option<&Expression>;
}

impl Fields for TableConstructor {
    fn named_field(&self, name: String) -> Option<&Expression> {
        self.fields().into_iter().find_map(|field| match field {
            full_moon::ast::Field::NameKey { key, value, .. } => {
                (key.to_string() == name).then(|| value)
            }
            _ => None,
        })
    }

    fn named_fields(&self) -> Vec<(String, &Expression)> {
        self.fields()
            .into_iter()
            .filter_map(|field| match field {
                full_moon::ast::Field::NameKey { key, value, .. } => Some((key.to_string(), value)),
                _ => None,
            })
            .collect()
    }

    fn indexed_fields(&self) -> Vec<&Expression> {
        self.fields()
            .into_iter()
            .filter_map(|field| match field {
                full_moon::ast::Field::NoKey(value) => Some(value),
                _ => None,
            })
            .collect()
    }

    fn indexed_field(&self, index: usize) -> Option<&Expression> {
        self.fields()
            .into_iter()
            .filter_map(|field| match field {
                full_moon::ast::Field::NoKey(val) => Some(val),
                _ => None,
            })
            .nth(index)
    }

    fn expr_fields(&self) -> Vec<(&Expression, &Expression)> {
        self.fields()
            .into_iter()
            .filter_map(|field| match field {
                full_moon::ast::Field::ExpressionKey { key, value, .. } => Some((key, value)),
                _ => None,
            })
            .collect()
    }

    fn expr_field(&self, index: &Expression) -> Option<&Expression> {
        self.fields().into_iter().find_map(|field| match field {
            full_moon::ast::Field::ExpressionKey { key, value, .. } => {
                (key == index).then(|| value)
            }
            _ => None,
        })
    }
}

impl Visitor for ScopeManager {
    fn visit_block(&mut self, block: &full_moon::ast::Block) {
        self.open_scope(block);
    }

    fn visit_block_end(&mut self, _node: &full_moon::ast::Block) {
        self.close_scope();
    }

    fn visit_local_function(&mut self, func: &full_moon::ast::LocalFunction) {
        let Some(scope) = self.get_current_scope_mut() else {
			return
		};
        let name = func.name().to_string();
        let body = func.body();
        scope.alloc_local(
            name.clone(),
            Value::Function((body.end_token().clone(), body.clone())),
        );
        self.name_next_scope(name);
    }

    fn visit_function_declaration(&mut self, node: &FunctionDeclaration) {
        let Some(global_id) = self.stack.first() else {
			return
		};
        let Some(scope) = self.scopes.get_mut(*global_id) else {
			return
		};
        let name = node.name().to_string().trim().to_string();
        let body = node.body();
        scope.alloc_local(
            name.clone(),
            Value::Function((body.end_token().clone(), body.clone())),
        );
        self.stack.push(*global_id);
        self.name_next_scope(name);
    }

    fn visit_function_declaration_end(&mut self, _node: &FunctionDeclaration) {
        self.stack.pop();
    }

    fn visit_assignment(&mut self, node: &full_moon::ast::Assignment) {
        let Some(global_id) = self.stack.first().copied() else {
			return
		};

        self.stack.push(global_id);
        node.variables()
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .zip(node.expressions().into_iter())
            .for_each(|(v, e)| match v {
                full_moon::ast::Var::Name(name) => match e {
                    Expression::Value { value } => {
                        let Some(scope) = self.scopes.get_mut(global_id) else {
                			return
                		};
                        let name = name.to_string().trim().to_string();
                        scope.alloc_local(name.clone(), *value.clone());
                        self.name_next_scope(name);
                    }
                    _ => {}
                },
                _ => {}
            });
    }

    fn visit_assignment_end(&mut self, _node: &full_moon::ast::Assignment) {
        self.close_scope();
    }

    fn visit_local_assignment(&mut self, node: &full_moon::ast::LocalAssignment) {
        node.names()
            .into_iter()
            .zip(node.expressions().into_iter())
            .for_each(|(name, e)| match e {
                Expression::Value { value } => {
                    let Some(scope) = self.get_current_scope_mut() else {
                			return
                		};
                    let name = name.to_string().trim().to_string();
                    scope.alloc_local(name.clone(), *value.clone());
                    self.name_next_scope(name);
                }
                _ => {}
            });
    }

    fn visit_local_assignment_end(&mut self, _node: &full_moon::ast::LocalAssignment) {}

    fn visit_function_call(&mut self, node: &FunctionCall) {
        let params = match node.prefix() {
            full_moon::ast::Prefix::Name(n) => {
                let name = n.to_string().trim().to_string();
                let curr_scope = self.get_current_scope_id().unwrap();

                let var = self
                    .find_var(&name)
                    .and_then(|var| match var {
                        Var::Local(val) => Some((curr_scope, val)),
                        Var::Reference(scope, var) => self.resolve_reference(scope, var),
                    })
                    .and_then(|(scope, val)| {
                        let val = self.get_value(scope, val)?;
                        match val {
                            Value::Function((_, f)) => Some(f),
                            _ => None,
                        }
                    });

                if let Some(var) = var {
                    var.parameters()
                        .iter()
                        .map(|p| {
                            (
                                p.to_string().trim().to_string(),
                                p.start_position().unwrap_or_default(),
                            )
                        })
                        .collect::<Vec<_>>()
                } else {
                    return;
                }
            }
            full_moon::ast::Prefix::Expression(expr) => {
                let Some(params) = self.extract_params(expr) else {
					return;
				};
                params
            }
            _ => return,
        };

        node.suffixes().next().map(|s| match s {
            Suffix::Call(Call::AnonymousCall(FunctionArgs::Parentheses { arguments, .. })) => {
                arguments
                    .iter()
                    .zip(params)
                    .map(|(param, (name, _))| (name, param.start_position().unwrap_or_default()))
                    .for_each(|(name, pos)| {
                        self.hints.push(InlayHint {
                            position: lsp_types::Position {
                                line: pos.line() as u32,
                                character: pos.character() as u32,
                            },
                            label: InlayHintLabel::String(name),
                            kind: Some(InlayHintKind::PARAMETER),
                            text_edits: None,
                            tooltip: None,
                            padding_left: None,
                            padding_right: None,
                            data: None,
                        });
                    });
            }
            _ => {}
        });
    }
}
