use std::collections::HashMap;

use pest::Parser;
use pest_derive::Parser;

use crate::query::ast::{
    BinaryOp, Direction, Expression, MatchClause, NodePattern, Pattern, RelationshipLength,
    RelationshipPattern, ReturnClause, ReturnItem, SetClause, Statement, WhereClause,
};

#[derive(Parser)]
#[grammar = "query/cypher.pest"]
pub struct CypherParser;

/// Parses a Cypher query string into a `Statement` AST.
pub fn parse(input: &str) -> Result<Statement, String> {
    let pairs =
        CypherParser::parse(Rule::statement, input).map_err(|e| format!("Parse error: {}", e))?;

    for pair in pairs {
        let inner = pair.into_inner().next().ok_or("Empty statement")?;
        return build_statement(inner);
    }

    Err("No statement found".to_string())
}

/// Dispatches a pest parse pair to the appropriate statement builder.
fn build_statement(pair: pest::iterators::Pair<Rule>) -> Result<Statement, String> {
    match pair.as_rule() {
        Rule::create_stmt => {
            let inner = pair.into_inner().next().ok_or("Expected node pattern")?;
            let node = build_pattern(inner)?;
            Ok(Statement::Create(node))
        }
        Rule::match_stmt => {
            let mut inner = pair.into_inner();

            let pattern_pair = inner.next().ok_or("Expected pattern")?;
            let pattern = build_pattern(pattern_pair)?;

            let mut where_clause = None;
            let mut return_clause = None;

            for p in inner {
                match p.as_rule() {
                    Rule::where_clause => {
                        let cond = build_expression(p.into_inner().next().unwrap())?;
                        where_clause = Some(WhereClause { condition: cond });
                    }
                    Rule::return_clause => {
                        return_clause = Some(build_return_clause(p)?);
                    }
                    _ => {}
                }
            }

            Ok(Statement::Match(Box::new(MatchClause {
                pattern,
                where_clause,
                return_clause: return_clause.ok_or("Missing RETURN clause")?,
            })))
        }
        Rule::delete_stmt => {
            let var = pair.into_inner().next().ok_or("Expected variable")?;
            Ok(Statement::Delete(var.as_str().to_string()))
        }
        Rule::set_stmt => {
            let inner = pair.into_inner().next().ok_or("Expected SET item")?;
            build_set_clause(inner)
        }
        _ => Err(format!("Unexpected rule: {:?}", pair.as_rule())),
    }
}

/// Builds a `SET` clause AST node from a `set_item` pair.
fn build_set_clause(pair: pest::iterators::Pair<Rule>) -> Result<Statement, String> {
    let mut inner = pair.into_inner();

    let var = inner
        .next()
        .ok_or("Expected variable")?
        .as_str()
        .to_string();
    let prop = inner
        .next()
        .ok_or("Expected property")?
        .as_str()
        .to_string();
    let value_pair = inner.next().ok_or("Expected value")?;
    let value = build_expression(value_pair)?;

    Ok(Statement::Set(Box::new(SetClause {
        variable: var,
        property: prop,
        value,
    })))
}

/// Builds a `Pattern` (single node or path) from a pattern pair.
fn build_pattern(pair: pest::iterators::Pair<Rule>) -> Result<Pattern, String> {
    let mut inner = pair.into_inner();

    let first_node = inner.next().ok_or("Expected node pattern")?;
    let node = build_node_pattern(first_node)?;

    if let Some(rel_pair) = inner.next() {
        let rel = build_relationship_pattern(rel_pair)?;
        let second_node = inner.next().ok_or("Expected second node pattern")?;
        let second = build_node_pattern(second_node)?;
        Ok(Pattern::Edge(
            Box::new(node),
            Box::new(rel),
            Box::new(second),
        ))
    } else {
        Ok(Pattern::Node(node))
    }
}

/// Builds a `NodePattern` from a `node_pattern` pair, extracting variable,
/// label, and property map.
fn build_node_pattern(pair: pest::iterators::Pair<Rule>) -> Result<NodePattern, String> {
    let mut inner = pair.into_inner();

    let var = inner
        .next()
        .ok_or("Expected variable")?
        .as_str()
        .to_string();

    let mut label = None;
    let mut properties = HashMap::new();

    for p in inner {
        match p.as_rule() {
            Rule::identifier => {
                label = Some(p.as_str().to_string());
            }
            Rule::property_map => {
                properties = build_property_map(p)?;
            }
            _ => {}
        }
    }

    Ok(NodePattern {
        variable: Some(var),
        label,
        properties,
    })
}

/// Builds a `RelationshipPattern` from a relationship pattern pair,
/// extracting direction, variable, type, and properties.
fn build_relationship_pattern(
    pair: pest::iterators::Pair<Rule>,
) -> Result<RelationshipPattern, String> {
    let inner = pair
        .into_inner()
        .next()
        .ok_or("Expected rel pattern variant")?;
    let rule = inner.as_rule();

    let direction = match rule {
        Rule::left_arrow_rel => Direction::Incoming,
        Rule::right_arrow_rel => Direction::Outgoing,
        Rule::undirected_rel => Direction::Undirected,
        _ => return Err(format!("Unexpected rel rule: {:?}", rule)),
    };

    let detail = inner
        .into_inner()
        .filter(|p| p.as_rule() == Rule::rel_detail)
        .next()
        .ok_or("Expected rel_detail")?;

    let detail_inner = detail.into_inner();

    let mut variable = None;
    let mut rel_type = None;
    let mut properties = HashMap::new();
    let mut hops = None;

    for p in detail_inner {
        match p.as_rule() {
            Rule::variable => {
                variable = Some(p.as_str().to_string());
            }
            Rule::identifier => {
                rel_type = Some(p.as_str().to_string());
            }
            Rule::property_map => {
                properties = build_property_map(p)?;
            }
            Rule::rel_length => {
                hops = Some(get_hops_range(p)?);
            }
            _ => {}
        }
    }

    Ok(RelationshipPattern {
        variable,
        rel_type,
        direction,
        hops,
        properties,
    })
}

fn get_hops_range(pair: pest::iterators::Pair<Rule>) -> Result<RelationshipLength, String> {
    let spec = pair
        .as_str()
        .strip_prefix('*')
        .ok_or("Invalid relationship length")?;

    if spec.is_empty() {
        return Ok(RelationshipLength {
            min_hops: None,
            max_hops: None,
        });
    }

    if let Some((min_s, max_s)) = spec.split_once("..") {
        let min_hops = if min_s.is_empty() {
            None
        } else {
            Some(
                min_s
                    .parse::<u32>()
                    .map_err(|e| format!("Invalid min hops: {e}"))?,
            )
        };

        let max_hops = if max_s.is_empty() {
            None
        } else {
            Some(
                max_s
                    .parse::<u32>()
                    .map_err(|e| format!("Invalid max hops: {e}"))?,
            )
        };

        if let (Some(min), Some(max)) = (min_hops, max_hops) {
            if min > max {
                return Err("Invalid relationship range: min cannot exceed max".to_string());
            }
        }

        return Ok(RelationshipLength { min_hops, max_hops });
    }

    let exact = spec
        .parse::<u32>()
        .map_err(|e| format!("Invalid hop count: {e}"))?;

    Ok(RelationshipLength {
        min_hops: Some(exact),
        max_hops: Some(exact),
    })
}
/// Builds a `HashMap<String, Expression>` from a `property_map` pair.
fn build_property_map(
    pair: pest::iterators::Pair<Rule>,
) -> Result<HashMap<String, Expression>, String> {
    let mut map = HashMap::new();
    for pair_item in pair.into_inner() {
        let mut prop_inner = pair_item.into_inner();
        let key = prop_inner
            .next()
            .ok_or("Expected property key")?
            .as_str()
            .to_string();
        let value_pair = prop_inner.next().ok_or("Expected property value")?;
        let value = build_expression(value_pair)?;
        map.insert(key, value);
    }
    Ok(map)
}

/// Builds a `ReturnClause` from a `return_clause` pair.
fn build_return_clause(pair: pest::iterators::Pair<Rule>) -> Result<ReturnClause, String> {
    let mut items = Vec::new();
    for item_pair in pair.into_inner() {
        let mut inner = item_pair.into_inner();
        let expr_pair = inner.next().ok_or("Expected return expression")?;
        let expression = build_expression(expr_pair)?;
        let alias = inner.next().map(|a| a.as_str().to_string());
        items.push(ReturnItem { expression, alias });
    }
    Ok(ReturnClause { items })
}

/// Recursively builds an `Expression` from a pest pair, handling literals,
/// comparisons, boolean operators, property access, and parenthesized expressions.
fn build_expression(pair: pest::iterators::Pair<Rule>) -> Result<Expression, String> {
    match pair.as_rule() {
        Rule::expression | Rule::or_expr => {
            let mut inner = pair.into_inner();
            let first = build_expression(inner.next().unwrap())?;
            inner.try_fold(first, |left, right| -> Result<Expression, String> {
                Ok(Expression::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOp::Or,
                    right: Box::new(build_expression(right)?),
                })
            })
        }
        Rule::and_expr => {
            let mut inner = pair.into_inner();
            let first = build_expression(inner.next().unwrap())?;
            inner.try_fold(first, |left, right| -> Result<Expression, String> {
                Ok(Expression::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOp::And,
                    right: Box::new(build_expression(right)?),
                })
            })
        }
        Rule::not_expr => {
            let mut not_count = 0usize;
            let mut base_expr: Option<Expression> = None;

            for p in pair.into_inner() {
                if p.as_str() == "NOT" {
                    not_count += 1;
                } else {
                    base_expr = Some(build_expression(p)?);
                }
            }

            let mut expr = base_expr.ok_or("Expected expression after NOT")?;

            for _ in 0..not_count {
                expr = Expression::UnaryOp {
                    op: crate::query::ast::UnaryOp::Not,
                    expr: Box::new(expr),
                };
            }

            Ok(expr)
        }
        Rule::comparison => {
            let mut inner = pair.into_inner();
            let left = build_expression(inner.next().unwrap())?;
            if let Some(op_pair) = inner.next() {
                let op = match op_pair.as_str() {
                    "=" => BinaryOp::Eq,
                    "<>" => BinaryOp::Neq,
                    ">" => BinaryOp::Gt,
                    ">=" => BinaryOp::Gte,
                    "<" => BinaryOp::Lt,
                    "<=" => BinaryOp::Lte,
                    _ => return Err(format!("Unknown operator: {}", op_pair.as_str())),
                };
                let right = build_expression(inner.next().unwrap())?;
                Ok(Expression::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                })
            } else {
                Ok(left)
            }
        }
        Rule::boolean => match pair.as_str() {
            "true" => Ok(Expression::Boolean(true)),
            "false" => Ok(Expression::Boolean(false)),
            _ => Err(format!("Invalid boolean: {}", pair.as_str())),
        },
        Rule::integer => Ok(Expression::Integer(
            pair.as_str()
                .parse()
                .map_err(|e| format!("Invalid integer: {}", e))?,
        )),
        Rule::float => Ok(Expression::Float(
            pair.as_str()
                .parse()
                .map_err(|e| format!("Invalid float: {}", e))?,
        )),
        Rule::string => {
            let s = pair.as_str();
            let unquoted = &s[1..s.len() - 1];
            Ok(Expression::String(unquoted.to_string()))
        }
        Rule::property_access => {
            let mut inner = pair.into_inner();
            let variable = inner
                .next()
                .ok_or("Expected variable")?
                .as_str()
                .to_string();
            let property = inner
                .next()
                .ok_or("Expected property")?
                .as_str()
                .to_string();
            Ok(Expression::Property { variable, property })
        }
        Rule::variable => Ok(Expression::Variable(pair.as_str().to_string())),

        Rule::paren_expr => {
            let inner = pair.into_inner().next().ok_or("Expected expression")?;
            build_expression(inner)
        }
        _ => Err(format!(
            "Unexpected rule in expression: {:?}",
            pair.as_rule()
        )),
    }
}
