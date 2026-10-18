mod runtime_env;
mod value;

use std::rc::Rc;

use runtime_env::{EnvRef, RuntimeEnv};
pub use value::{FunctionValue, Value};

use crate::{ast::{AndExpr, Assignment, Block, Closure, Declaration, Defun, Expr, Funcall, IfExpr, Literal, LiteralType, NotExpr, OrExpr, Program}, diagnostics::Diagnostic, parser::{Parser, Position}};

#[derive(Debug, Clone)]
pub struct Interpreter {
    pub file: String,
    pub source_code: String,
    pub diagnotics: Vec<Diagnostic>,
    global_env: EnvRef,
}

impl Interpreter {
    pub fn new(file: String, source_code: String) -> Self {
        Self {
            file: file,
            source_code: source_code,
            diagnotics: vec![],
            global_env: RuntimeEnv::new(None),
        }
    }

    pub fn eval_program(&mut self, program: &Program) -> Result<Value, Vec<Diagnostic>> {
        let mut last_value = Value::Unit;

        for expr in &program.expressions {
            last_value = self.eval_expr(expr, self.global_env.clone());
        }

        Ok(last_value)
    }

    fn eval_expr(&mut self, expr: &Expr, env: EnvRef) -> Value {
        match expr {
            Expr::Literal(literal) => self.eval_literal(literal),
            Expr::Variable(variable) => self.eval_variable(variable.position.clone(), &variable.value, env),
            Expr::Defun(defun) => self.eval_defun(defun, env),
            Expr::Declaration(declaration) => self.eval_declaration(declaration, env),
            Expr::Assignment(assignment) => self.eval_assignment(assignment, env),
            Expr::Block(block) => self.eval_block(block, Some(env)),
            Expr::Funcall(funcall) => self.eval_funcall(funcall, env),
            Expr::Closure(closure) => self.eval_closure(closure, env),
            Expr::IfExpr(if_expr) => self.eval_if(if_expr, env),
            Expr::Or(or_expr) => self.eval_or(or_expr, env),
            Expr::And(and_expr) => self.eval_and(and_expr, env),
            Expr::Not(not_expr) => self.eval_not(not_expr, env),
            Expr::Error => unreachable!(),
        }
    }

    fn builtin_funcall_name<'a>(&self, funcall: &'a Funcall) -> Option<&'a str> {
        let Expr::Variable(variable) = funcall.callee.as_ref() else {
            return None;
        };

        match variable.value.as_str() {
            "+" | "-" | "*" | "/" | "=" | "<" | "<=" | ">" | ">=" => Some(variable.value.as_str()),
            _ => None,
        }
    }

    fn eval_literal(&self, literal: &Literal) -> Value {
        match literal.ty {
            LiteralType::Int => literal.value.parse().map(Value::Int).unwrap_or(Value::Unit),
            LiteralType::Float => literal.value.parse().map(Value::Float).unwrap_or(Value::Unit),
            LiteralType::Bool => Value::Bool(literal.value == "true"),
            LiteralType::String => Value::String(literal.value.clone()),
            LiteralType::Unit => Value::Unit,
        }
    }

    fn eval_variable(&mut self, position: Position, name: &str, env: EnvRef) -> Value {
        let _ = position;
        RuntimeEnv::get(&env, name).expect("type checker should resolve variables before evaluation").value
    }

    fn eval_defun(&mut self, defun: &Defun, env: EnvRef) -> Value {
        let function = self.eval_closure(&defun.closure, env.clone());
        RuntimeEnv::define(&env, defun.name.value.clone(), function, false);
        Value::Unit
    }

    fn eval_declaration(&mut self, declaration: &Declaration, env: EnvRef) -> Value {
        let value = self.eval_expr(&declaration.value, env.clone());
        RuntimeEnv::define(&env, declaration.signature.name.value.clone(), value, declaration.mutable);
        Value::Unit
    }

    fn eval_assignment(&mut self, assignment: &Assignment, env: EnvRef) -> Value {
        let value = self.eval_expr(&assignment.value, env.clone());
        RuntimeEnv::assign(&env, &assignment.variable.value, value);
        Value::Unit
    }

    fn eval_block(&mut self, block: &Block, parent: Option<EnvRef>) -> Value {
        let env =RuntimeEnv::new(parent);
        let mut last_value = Value::Unit;

        for expr in &block.contents {
            last_value = self.eval_expr(expr, env.clone());
        }

        last_value
    }

    fn eval_funcall(&mut self, funcall: &Funcall, env: EnvRef) -> Value {
        if let Some(name) = self.builtin_funcall_name(funcall) {
            return self.eval_builtin_funcall(funcall, name, env);
        }

        let callee = self.eval_expr(&funcall.callee, env.clone());
        let Value::Function(function) = callee else {
            unreachable!("type checker should ensure callees are functions");
        };

        let mut args = vec![];
        for arg in funcall.args.iter().take(function.params.len()) {
            args.push(self.eval_expr(arg, env.clone()));
        }

        let call_env = RuntimeEnv::new(Some(function.env.clone()));
        for (param, arg) in function.params.iter().zip(args.into_iter()) {
            RuntimeEnv::define(&call_env, param.clone(), arg, false);
        }

        self.eval_block(&function.body, Some(call_env))
    }

    fn eval_builtin_funcall(&mut self, funcall: &Funcall, name: &str, env: EnvRef) -> Value {
        let args: Vec<Value> = funcall.args.iter().map(|arg| self.eval_expr(arg, env.clone())).collect();

        match name {
            "+" => self.eval_numeric_builtin(funcall.position.clone(), args, |x, y| x + y, |x, y| x + y),
            "-" => self.eval_numeric_builtin(funcall.position.clone(), args, |x, y| x - y, |x, y| x - y),
            "*" => self.eval_numeric_builtin(funcall.position.clone(), args, |x, y| x * y, |x, y| x * y),
            "/" => self.eval_numeric_builtin(funcall.position.clone(), args, |x, y| x / y, |x, y| x / y),
            "=" => self.eval_equality_builtin(funcall.position.clone(), args),
            "<" => self.eval_ordering_builtin(funcall.position.clone(), args, |x, y| x < y, |x, y| x < y),
            "<=" => self.eval_ordering_builtin(funcall.position.clone(), args, |x, y| x <= y, |x, y| x <= y),
            ">" => self.eval_ordering_builtin(funcall.position.clone(), args, |x, y| x > y, |x, y| x > y),
            ">=" => self.eval_ordering_builtin(funcall.position.clone(), args, |x, y| x >= y, |x, y| x >= y),
            _ => unreachable!(),
        }
    }

    fn eval_numeric_builtin(&mut self, position: Position, args: Vec<Value>, int_op: impl Fn(i64, i64) -> i64, float_op: impl Fn(f64, f64) -> f64) -> Value {
        let _ = position;
        match args.first() {
            Some(Value::Int(first)) => {
                let mut acc = *first;

                for arg in &args[1..] {
                    let Value::Int(value) = arg else { unreachable!("type checker should ensure homogeneous integer arithmetic"); };

                    acc = int_op(acc, *value);
                }

                Value::Int(acc)
            }
            Some(Value::Float(first)) => {
                let mut acc = *first;

                for arg in &args[1..] {
                    let Value::Float(value) = arg else { unreachable!("type checker should ensure homogeneous float arithmetic"); };

                    acc = float_op(acc, *value);
                }

                Value::Float(acc)
            }
            Some(_) => unreachable!("type checker should ensure arithmetic operands are numeric"),
            None => unreachable!("type checker should ensure arithmetic has enough arguments"),
        }
    }

    fn eval_equality_builtin(&mut self, _position: Position, args: Vec<Value>) -> Value {
        match args.as_slice() {
            [left, right] => match (left, right) {
                (Value::Int(x), Value::Int(y)) => Value::Bool(x == y),
                (Value::Float(x), Value::Float(y)) => Value::Bool(x == y),
                (Value::Bool(x), Value::Bool(y)) => Value::Bool(x == y),
                (Value::String(x), Value::String(y)) => Value::Bool(x == y),
                (Value::Unit, Value::Unit) => Value::Bool(true),
                (Value::Function(_), Value::Function(_)) => unreachable!("type checker should reject function equality"),
                _ => unreachable!("type checker should ensure equality operands have the same type"),
            },
            _ => unreachable!("type checker should ensure equality arity"),
        }
    }

    fn eval_ordering_builtin(&mut self, position: Position, args: Vec<Value>, int_op: impl FnOnce(i64, i64) -> bool, float_op: impl FnOnce(f64, f64) -> bool) -> Value {
        let _ = position;
        match args.as_slice() {
            [Value::Int(left), Value::Int(right)] => Value::Bool(int_op(*left, *right)),
            [Value::Float(left), Value::Float(right)] => Value::Bool(float_op(*left, *right)),
            [_, _] => unreachable!("type checker should ensure comparable operand types"),
            _ => unreachable!("type checker should ensure comparison arity"),
        }
    }

    fn eval_closure(&mut self, closure: &Closure, env: EnvRef) -> Value {
        Value::Function(Rc::new(FunctionValue {
            params: closure.params.iter().map(|param| param.name.value.clone()).collect(),
            body: closure.body.clone(),
            env,
        }))
    }

    fn eval_if(&mut self, if_expr: &IfExpr, env: EnvRef) -> Value {
        for branch in &if_expr.branches {
            match self.eval_expr(&branch.condition, env.clone()) {
                Value::Bool(true) => return self.eval_block(&branch.block, Some(env.clone())),
                Value::Bool(false) => continue,
                _ => unreachable!("type checker should ensure if conditions are booleans"),
            }
        }

        if let Some(else_branch) = &if_expr.else_branch {
            self.eval_expr(else_branch, env)
        } else {
            Value::Unit
        }
    }

    fn eval_or(&mut self, or_expr: &OrExpr, env: EnvRef) -> Value {
        for condition in &or_expr.conditions {
            match self.eval_expr(condition, env.clone()) {
                Value::Bool(true) => return Value::Bool(true),
                Value::Bool(false) => {}
                _ => unreachable!("type checker should ensure 'or' operands are booleans"),
            }
        }

        Value::Bool(false)
    }

    fn eval_and(&mut self, and_expr: &AndExpr, env: EnvRef) -> Value {
        for condition in &and_expr.conditions {
            match self.eval_expr(condition, env.clone()) {
                Value::Bool(true) => {}
                Value::Bool(false) => return Value::Bool(false),
                _ => unreachable!("type checker should ensure 'and' operands are booleans"),
            }
        }

        Value::Bool(true)
    }

    fn eval_not(&mut self, not_expr: &NotExpr, env: EnvRef) -> Value {
        match self.eval_expr(&not_expr.condition, env) {
            Value::Bool(value) => Value::Bool(!value),
            _ => unreachable!("type checker should ensure 'not' operand is a boolean"),
        }
    }
}
