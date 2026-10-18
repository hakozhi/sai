mod keyword;
mod lexer;
mod position;
mod validator;

use keyword::{Keyword, parse_keyword};
use lexer::TokenType;
pub use lexer::{Lexer, Token};
pub use position::Position;
use validator::ASTValidator;

use crate::{
    ast::{
        AndExpr, Assignment, Block, Branch, Closure, Declaration, Defun, Expr, Funcall, IfExpr, Literal, LiteralType, NotExpr, OrExpr, Parameter, Program, TypeAnnotation, TypeExpr, Variable,
        VariableSignature,
    },
    diagnostics::Diagnostic,
};

#[derive(Debug, Clone)]
pub struct Parser {
    file: String,
    tokens: Vec<Token>,
    index: usize,
    current_token: Option<Token>,
    diagnostics: Vec<Diagnostic>,
}

type ParseResult<T> = Result<T, Vec<Diagnostic>>;
type ClosureSignature = (Vec<Parameter>, TypeAnnotation);

impl Parser {
    pub fn new(file: &str, tokens: Vec<Token>) -> Self {
        let current_token = tokens.get(0).cloned();
        Parser {
            file: file.to_string(),
            tokens: tokens,
            index: 0,
            current_token: current_token,
            diagnostics: vec![],
        }
    }

    pub fn parse(&mut self) -> ParseResult<Program> {
        match self.read_all_tokentrees() {
            Ok(tokentrees) => {
                let mut program = Program {
                    name: self.file.clone(),
                    expressions: vec![],
                };

                for tokentree in tokentrees {
                    program.expressions.push(self.parse_tokentree(&tokentree));
                }

                if self.diagnostics.is_empty() {
                    let mut validator = ASTValidator::new();
                    if let Err(mut e) = validator.validate(&program) {
                        self.diagnostics.append(&mut e);
                        Err(self.diagnostics.clone())
                    } else {
                        Ok(program)
                    }
                } else {
                    Err(self.diagnostics.clone())
                }
            }
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);

                Err(self.diagnostics.clone())
            }
        }
    }

    fn parse_tokentree(&mut self, tokentree: &TokenTree) -> Expr {
        match tokentree {
            TokenTree::Token(token) => self.parse_token(token),
            TokenTree::List(list) => {
                if list.items.is_empty() {
                    self.add_diagnostic(list.position.clone(), "Empty list");
                    Expr::Error
                } else {
                    let head = &list.items[0];

                    if let TokenTree::Token(token) = head {
                        match token.ty {
                            TokenType::Keyword => self.parse_special_forms(&token.value, list),
                            TokenType::Symbol => self.parse_funcall(list),
                            TokenType::IntLiteral | TokenType::FloatLiteral | TokenType::StringLiteral | TokenType::BoolLiteral => {
                                self.add_diagnostic(token.position.clone(), "Unexpected literal");
                                Expr::Error
                            }
                            _ => {
                                self.add_diagnostic(token.position.clone(), "Unexpected token");
                                Expr::Error
                            }
                        }
                    } else {
                        self.parse_funcall(list)
                    }
                }
            }
        }
    }

    fn parse_token(&mut self, token: &Token) -> Expr {
        match token.ty {
            TokenType::LParen | TokenType::RParen => {
                self.add_diagnostic(token.position.clone(), "Unexpected paren");
                Expr::Error
            }
            TokenType::Keyword => {
                self.add_diagnostic(token.position.clone(), "Unexpected keyword");
                Expr::Error
            }
            TokenType::Symbol => Expr::Variable(Variable {
                position: token.position.clone(),
                value: token.value.clone(),
            }),
            _ => self.parse_literal(token.clone()),
        }
    }

    fn parse_literal(&mut self, token: Token) -> Expr {
        let mut literal = Literal {
            position: token.position,
            value: token.value,
            ty: LiteralType::Unit,
        };

        match token.ty {
            TokenType::IntLiteral => literal.ty = LiteralType::Int,
            TokenType::FloatLiteral => literal.ty = LiteralType::Float,
            TokenType::BoolLiteral => literal.ty = LiteralType::Bool,
            TokenType::StringLiteral => literal.ty = LiteralType::String,
            _ => {
                self.add_diagnostic(literal.position.clone(), "expeceted literal");
                return Expr::Error;
            }
        };

        Expr::Literal(literal)
    }

    fn parse_special_forms(&mut self, keyword: &str, tokentree: &TokenTreeList) -> Expr {
        match parse_keyword(keyword) {
            Keyword::Block => self.parse_block(tokentree),
            Keyword::Def => self.parse_defun(tokentree),
            Keyword::Let => self.parse_declaration(tokentree, false),
            Keyword::Mut => self.parse_declaration(tokentree, true),
            Keyword::Set => self.parse_assignment(tokentree),
            Keyword::If => self.parse_if(tokentree),
            Keyword::Closure => self.parse_closure(tokentree),
            Keyword::Or => self.parse_or(tokentree),
            Keyword::And => self.parse_and(tokentree),
            Keyword::Not => self.parse_not(tokentree),
            Keyword::Else | Keyword::Elif => {
                self.add_diagnostic(tokentree.position.clone(), "Unexpected keyword");
                Expr::Error
            }
        }
    }

    fn parse_funcall(&mut self, tokenlist: &TokenTreeList) -> Expr {
        let mut funcall = Funcall {
            position: tokenlist.position.clone(),
            callee: Box::new(self.parse_tokentree(&tokenlist.items[0])),
            args: vec![],
        };

        for arg in &tokenlist.items[1..] {
            funcall.args.push(self.parse_tokentree(arg));
        }

        Expr::Funcall(funcall)
    }

    fn parse_block(&mut self, tokenlist: &TokenTreeList) -> Expr {
        self.parse_expr_sequence_as_block(&tokenlist.items[1..], tokenlist.position.clone())
    }

    fn parse_expr_sequence_as_block(&mut self, items: &[TokenTree], position: Position) -> Expr {
        let mut block = Block { position: position, contents: vec![] };

        for item in items {
            block.contents.push(self.parse_tokentree(item));
        }

        Expr::Block(block)
    }

    // (def fib ((int n)) int ...)
    fn parse_defun(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() <= 3 {
            self.add_diagnostic(tokenlist.position.clone(), "improper form of defing a function");
            Expr::Error
        } else {
            let name = match self.parse_variable(&tokenlist.items[1]) {
                Ok(name) => name,
                Err(e) => {
                    self.diagnostics.push(e);
                    return Expr::Error;
                }
            };

            let closure = self.parse_closure_parts(tokenlist.position.clone(), &tokenlist.items[2], &tokenlist.items[3], &tokenlist.items[4..]);

            if let Expr::Closure(closure) = closure {
                Expr::Defun(Defun {
                    position: tokenlist.position.clone(),
                    name: name,
                    closure: closure,
                })
            } else {
                self.add_diagnostic(tokenlist.items[2].get_position(), "expected a closure definition");
                Expr::Error
            }
        }
    }

    fn parse_declaration(&mut self, tokenlist: &TokenTreeList, mutable: bool) -> Expr {
        if tokenlist.items.len() != 3 {
            self.add_diagnostic(tokenlist.position.clone(), "bad syntax for declaration");
            Expr::Error
        } else {
            let variable_signature = tokenlist.items[1].clone();
            let value = self.parse_tokentree(&tokenlist.items[2]);

            if let TokenTree::List(list) = variable_signature {
                let signature = match self.parse_variable_signature(&list) {
                    Ok(signature) => signature,
                    Err(mut e) => {
                        self.diagnostics.append(&mut e);
                        return Expr::Error;
                    }
                };

                Expr::Declaration(Declaration {
                    position: list.position.clone(),
                    signature: signature,
                    value: Box::new(value),
                    mutable: mutable,
                })
            } else {
                self.add_diagnostic(tokenlist.position.clone(), "bad syntax for declaration");
                Expr::Error
            }
        }
    }

    fn parse_variable_signature(&mut self, tokenlist: &TokenTreeList) -> Result<VariableSignature, Vec<Diagnostic>> {
        let items = tokenlist.items.as_slice();
        let mut diagnostics = vec![];

        match items {
            [ty, name] => match (self.parse_type(ty), self.parse_variable(name)) {
                (Ok(ty), Ok(name)) => Ok(VariableSignature { ty: ty, name: name }),
                (Err(e1), Err(e2)) => {
                    diagnostics.push(e1);
                    diagnostics.push(e2);
                    Err(diagnostics)
                }
                (Err(e), _) => {
                    diagnostics.push(e);
                    Err(diagnostics)
                }
                (_, Err(e)) => {
                    diagnostics.push(e);
                    Err(diagnostics)
                }
            },
            _ => {
                diagnostics.push(Diagnostic::new(tokenlist.position.clone(), "improper form of variable signature"));

                Err(diagnostics)
            }
        }
    }

    fn parse_type(&mut self, tokentree: &TokenTree) -> Result<TypeAnnotation, Diagnostic> {
        if let Ok(ty) = self.parse_variable(tokentree) {
            Ok(TypeAnnotation {
                position: ty.position,
                value: TypeExpr::Named(ty.value),
            })
        } else {
            Err(Diagnostic::new(tokentree.get_position(), "improper form of type"))
        }
    }

    fn parse_variable(&mut self, tokentree: &TokenTree) -> Result<Variable, Diagnostic> {
        if let Expr::Variable(var) = self.parse_tokentree(tokentree) {
            Ok(var)
        } else {
            Err(Diagnostic::new(tokentree.get_position(), "expected variable"))
        }
    }

    fn parse_assignment(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() != 3 {
            self.add_diagnostic(tokenlist.position.clone(), "bad syntax for assignment");
            Expr::Error
        } else {
            let var = self.parse_tokentree(&tokenlist.items[1]);

            if let Expr::Variable(var) = var {
                let assignment = Assignment {
                    position: tokenlist.position.clone(),
                    variable: var,
                    value: Box::new(self.parse_tokentree(&tokenlist.items[2])),
                };

                Expr::Assignment(assignment)
            } else {
                self.add_diagnostic(tokenlist.position.clone(), "expected a variable");
                Expr::Error
            }
        }
    }

    // (fn ([int n]) <type> ...)
    fn parse_closure(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() <= 2 {
            self.add_diagnostic(tokenlist.position.clone(), "improper form of defining a closure");
            Expr::Error
        } else {
            self.parse_closure_parts(tokenlist.position.clone(), &tokenlist.items[1], &tokenlist.items[2], &tokenlist.items[3..])
        }
    }

    fn parse_closure_parts(&mut self, position: Position, parameters: &TokenTree, return_ty: &TokenTree, items: &[TokenTree]) -> Expr {
        let body = self.parse_expr_sequence_as_block(items, items[0].get_position());
        let TokenTree::List(parameters) = parameters else {
            self.add_diagnostic(parameters.get_position(), "unexpected token");
            return Expr::Error;
        };

        let parameters = match self.parse_closure_parameters(&parameters.items) {
            Ok(p) => p,
            Err(mut e) => {
                self.diagnostics.append(&mut e);
                return Expr::Error;
            }
        };

        let return_type = match self.parse_type(return_ty) {
            Ok(ty) => ty,
            Err(e) => {
                self.diagnostics.push(e);
                return Expr::Error;
            }
        };

        let Expr::Block(body) = body else { unreachable!() };

        Expr::Closure(Closure {
            position: position,
            params: parameters,
            return_type: return_type,
            body: body,
        })
    }

    fn parse_closure_parameters(&mut self, items: &Vec<TokenTree>) -> Result<Vec<Parameter>, Vec<Diagnostic>> {
        let mut parameters: Vec<Parameter> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        for item in items {
            match item {
                TokenTree::Token(_) => diagnostics.push(Diagnostic::new(item.get_position(), "expected parameter")),
                TokenTree::List(parameter_annotation) => {
                    match self.parse_parameter(parameter_annotation) {
                        Ok(parameter) => parameters.push(parameter),
                        Err(e) => diagnostics.push(e),
                    }
                }
            }
        }

        if diagnostics.is_empty() {
            Ok(parameters)
        }
        else {
            Err(diagnostics)
        }
    }

    fn parse_parameter(&mut self, tokenlist: &TokenTreeList) -> Result<Parameter, Diagnostic> {
        let items = tokenlist.items.as_slice();

        match items {
            [ty, name] => {
                let annotation = self.parse_type(ty)?;
                if let Expr::Variable(name) = self.parse_tokentree(name) {
                    Ok(Parameter {
                        position: tokenlist.position.clone(),
                        name: Variable {
                            position: name.position,
                            value: name.value,
                        },
                        ty: annotation,
                    })
                } else {
                    Err(Diagnostic::new(tokenlist.position.clone(), "expected name"))
                }
            }
            _ => Err(Diagnostic::new(tokenlist.position.clone(), "improper form of a parameter")),
        }
    }

    fn parse_if(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() < 3 {
            self.add_diagnostic(tokenlist.position.clone(), "bad syntax for if");
            return Expr::Error;
        }

        let mut if_expr = IfExpr {
            position: tokenlist.position.clone(),
            branches: vec![],
            else_branch: None,
        };

        let items = &tokenlist.items[1..];

        let mut current_branch_start = 0;
        let mut index = 0;
        let mut seen_else = false;
        let mut diagnostics = vec![];

        while index < items.len() {
            if let TokenTree::Token(token) = &items[index]
                && matches!(token.ty, TokenType::Keyword)
            {
                match parse_keyword(&token.value) {
                    Keyword::Elif => {
                        if seen_else {
                            diagnostics.push(Diagnostic::new(token.position.clone(), "'elif' cannot appear after 'else'"));
                        }

                        if index == current_branch_start {
                            diagnostics.push(Diagnostic::new(token.position.clone(), "missing branch before 'elif'"));
                        }

                        match self.parse_branch(&items[current_branch_start..index], items[current_branch_start].get_position()) {
                            Ok(branch) => if_expr.branches.push(branch),
                            Err(e) => diagnostics.push(e),
                        }

                        current_branch_start = index + 1;
                    }
                    Keyword::Else => {
                        if seen_else {
                            diagnostics.push(Diagnostic::new(token.position.clone(), "duplicate 'else' branch"));
                        }

                        if index == current_branch_start {
                            diagnostics.push(Diagnostic::new(token.position.clone(), "missing branch before 'else'"));
                        }

                        match self.parse_branch(&items[current_branch_start..index], items[current_branch_start].get_position()) {
                            Ok(branch) => if_expr.branches.push(branch),
                            Err(e) => diagnostics.push(e),
                        }

                        let else_items = &items[index + 1..];

                        if else_items.is_empty() {
                            diagnostics.push(Diagnostic::new(token.position.clone(), "'else' requires at least 1 body expression"));
                        } else {
                            let else_block = self.parse_expr_sequence_as_block(else_items, else_items[0].get_position());
                            if_expr.else_branch = Some(Box::new(else_block));

                            seen_else = true;

                            if index + 1 < items.len() {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }

            index += 1;
        }

        if !seen_else {
            let tail = &items[current_branch_start..];
            match self.parse_branch(tail, tail[0].get_position()) {
                Ok(branch) => if_expr.branches.push(branch),
                Err(e) => diagnostics.push(e),
            }
        }

        if diagnostics.is_empty() {
            Expr::IfExpr(if_expr)
        } else {
            self.diagnostics.append(&mut diagnostics);
            Expr::Error
        }
    }

    fn parse_branch(&mut self, items: &[TokenTree], position: Position) -> Result<Branch, Diagnostic> {
        if items.len() < 2 {
            return Err(Diagnostic::new(position, "branch requires a condition and at least one body expression"));
        }

        let condition = self.parse_tokentree(&items[0]);
        let body = self.parse_expr_sequence_as_block(&items[1..], items[1].get_position());

        if let Expr::Block(block) = body {
            Ok(Branch {
                condition: Box::new(condition),
                block: block,
            })
        } else {
            unreachable!()
        }
    }

    fn parse_or(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() <= 2 {
            self.add_diagnostic(tokenlist.position.clone(), "'or' requires at least 2 argument");
            Expr::Error
        } else {
            let mut or_operator = OrExpr {
                position: tokenlist.position.clone(),
                conditions: vec![],
            };

            for item in &tokenlist.items[1..] {
                or_operator.conditions.push(self.parse_tokentree(item));
            }

            Expr::Or(or_operator)
        }
    }

    fn parse_not(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() != 2 {
            self.add_diagnostic(tokenlist.position.clone(), "'not' requires only 1 argument.");
            Expr::Error
        } else {
            let not_operator = NotExpr {
                position: tokenlist.position.clone(),
                condition: Box::new(self.parse_tokentree(&tokenlist.items[1])),
            };

            Expr::Not(not_operator)
        }
    }

    fn parse_and(&mut self, tokenlist: &TokenTreeList) -> Expr {
        if tokenlist.items.len() <= 2 {
            self.add_diagnostic(tokenlist.position.clone(), "'and' requires at least 2 arguments");
            Expr::Error
        } else {
            let mut and_operator = AndExpr {
                position: tokenlist.position.clone(),
                conditions: vec![],
            };

            for item in &tokenlist.items[1..] {
                and_operator.conditions.push(self.parse_tokentree(item));
            }

            Expr::And(and_operator)
        }
    }

    fn read_all_tokentrees(&mut self) -> Result<Vec<TokenTree>, Diagnostic> {
        let mut tokentrees: Vec<TokenTree> = vec![];

        while self.current_token.is_some() {
            tokentrees.push(self.read_tokentree()?);
        }

        Ok(tokentrees)
    }

    fn read_tokentree(&mut self) -> Result<TokenTree, Diagnostic> {
        if let Some(current_token) = self.current_token.clone() {
            if matches!(current_token.ty, TokenType::LParen) {
                let left_paren_position = current_token.position;
                self.advance();

                let mut list: Vec<TokenTree> = vec![];

                while let Some(current_token) = self.current_token.clone() {
                    if matches!(current_token.ty, TokenType::RParen) {
                        self.advance();
                        return Ok(TokenTree::List(TokenTreeList {
                            position: left_paren_position,
                            items: list,
                        }));
                    }

                    list.push(self.read_tokentree()?);
                }

                Err(Diagnostic::new(left_paren_position, "'(' was never closed"))
            } else if matches!(current_token.ty, TokenType::RParen) {
                Err(Diagnostic::new(current_token.position, "unmatched ')'"))
            } else {
                self.advance();
                Ok(TokenTree::Token(current_token))
            }
        } else {
            unreachable!()
        }
    }

    fn add_diagnostic(&mut self, position: Position, msg: &str) {
        self.diagnostics.push(Diagnostic::new(position, msg));
    }

    fn advance(&mut self) {
        self.index += 1;
        self.current_token = self.tokens.get(self.index).cloned();
    }
}

#[derive(Debug, Clone)]
pub enum TokenTree {
    Token(Token),
    List(TokenTreeList),
}

#[derive(Debug, Clone)]
pub struct TokenTreeList {
    pub position: Position,
    pub items: Vec<TokenTree>,
}

impl TokenTree {
    pub fn get_position(&self) -> Position {
        match self {
            TokenTree::Token(token) => token.position.clone(),
            TokenTree::List(list) => list.position.clone(),
        }
    }
}
