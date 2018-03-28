use std::collections::HashMap;
use std::rc::Rc;
use syntax::expression::*;

use bit_vec::BitVec;
use engine::aggregation_operator::*;
use engine::aggregator::Aggregator;
use engine::filter::Filter;
use engine::typed_vec::TypedVec;
use engine::types::*;
use engine::vector_op::*;
use ingest::raw_val::RawVal;
use mem_store::column::Column;
use mem_store::column::{ColumnData, ColumnCodec};


#[derive(Debug)]
pub enum QueryPlan<'a> {
    GetDecode(&'a ColumnData),
    FilterDecode(&'a ColumnData, Rc<BitVec>),
    IndexDecode(&'a ColumnData, Rc<Vec<usize>>),
    GetEncoded(&'a ColumnCodec),
    FilterEncoded(&'a ColumnCodec, Rc<BitVec>),
    IndexEncoded(&'a ColumnCodec, Rc<Vec<usize>>),

    Decode(Box<QueryPlan<'a>>),

    EncodeStrConstant(Box<QueryPlan<'a>>, &'a ColumnCodec),
    EncodeIntConstant(Box<QueryPlan<'a>>, &'a ColumnCodec),

    LessThanVS(EncodingType, Box<QueryPlan<'a>>, Box<QueryPlan<'a>>),
    EqualsVS(EncodingType, Box<QueryPlan<'a>>, Box<QueryPlan<'a>>),
    And(Box<QueryPlan<'a>>, Box<QueryPlan<'a>>),
    Or(Box<QueryPlan<'a>>, Box<QueryPlan<'a>>),

    Constant(RawVal),
}

pub fn prepare(plan: QueryPlan) -> BoxedOperator {
    match plan {
        QueryPlan::GetDecode(col) => Box::new(GetDecode::new(col)),
        QueryPlan::FilterDecode(col, filter) => Box::new(FilterDecode::new(col, filter)),
        QueryPlan::IndexDecode(col, filter) => Box::new(IndexDecode::new(col, filter)),
        QueryPlan::GetEncoded(col) => Box::new(GetEncoded::new(col)),
        QueryPlan::FilterEncoded(col, filter) => Box::new(FilterEncoded::new(col, filter)),
        QueryPlan::IndexEncoded(col, filter) => Box::new(IndexEncoded::new(col, filter)),
        QueryPlan::Constant(ref c) => Box::new(Constant::new(c.clone())),
        QueryPlan::Decode(plan) => Box::new(Decode::new(prepare(*plan))),
        QueryPlan::EncodeStrConstant(plan, codec) => Box::new(EncodeStrConstant::new(prepare(*plan), codec)),
        QueryPlan::EncodeIntConstant(plan, codec) => Box::new(EncodeIntConstant::new(prepare(*plan), codec)),
        QueryPlan::LessThanVS(left_type, lhs, rhs) => VecOperator::less_than_vs(left_type, prepare(*lhs), prepare(*rhs)),
        QueryPlan::EqualsVS(left_type, lhs, rhs) => VecOperator::equals_vs(left_type, prepare(*lhs), prepare(*rhs)),
        QueryPlan::Or(lhs, rhs) => Boolean::or(prepare(*lhs), prepare(*rhs)),
        QueryPlan::And(lhs, rhs) => Boolean::and(prepare(*lhs), prepare(*rhs)),
    }
}

// TODO(clemens): add QueryPlan::Aggregation and merge with prepare function
pub fn prepare_aggregation<'a, 'b>(plan: QueryPlan<'a>,
                                   grouping: &'b TypedVec<'a>,
                                   max_index: usize,
                                   aggregator: Aggregator) -> Box<VecOperator<'a> + 'b> {
    match (aggregator, plan) {
        (Aggregator::Count, QueryPlan::Constant(RawVal::Int(_))) => match grouping.get_type() {
            EncodingType::U8 => Box::new(VecCount::new(grouping.cast_ref_u8().0, max_index, false)),
            EncodingType::U16 => Box::new(VecCount::new(grouping.cast_ref_u16().0, max_index, false)),
            t => panic!("unsupported type {:?} for grouping key", t),
        }
        (a, p) => panic!("prepare_aggregation not implemented for {:?}, {:?}", &a, &p)
    }
}


impl<'a> QueryPlan<'a> {
    pub fn create_query_plan<'b>(expr: &Expr,
                                 columns: &HashMap<&'b str, &'b Column>,
                                 filter: Filter) -> (QueryPlan<'b>, Type<'b>) {
        use self::Expr::*;
        use self::FuncType::*;
        match *expr {
            ColName(ref name) => match columns.get::<str>(name.as_ref()) {
                Some(c) => {
                    let t = c.data().full_type();
                    match (c.data().to_codec(), filter) {
                        (None, Filter::None) => (QueryPlan::GetDecode(c.data()), t.decoded()),
                        (None, Filter::BitVec(f)) => (QueryPlan::FilterDecode(c.data(), f), t.decoded()),
                        (None, Filter::Indices(f)) => (QueryPlan::IndexDecode(c.data(), f), t.decoded()),
                        (Some(c), Filter::None) => (QueryPlan::GetEncoded(c), t),
                        (Some(c), Filter::BitVec(f)) => (QueryPlan::FilterEncoded(c, f), t.mutable()),
                        (Some(c), Filter::Indices(f)) => (QueryPlan::IndexEncoded(c, f), t.mutable()),
                    }
                }
                None => panic!("Not implemented")//VecOperator::Constant(VecValue::Constant(RawVal::Null)),
            }
            Func(LT, ref lhs, ref rhs) => {
                let (plan_lhs, type_lhs) = QueryPlan::create_query_plan(lhs, columns, filter.clone());
                let (plan_rhs, type_rhs) = QueryPlan::create_query_plan(rhs, columns, filter);
                match (type_lhs.decoded, type_rhs.decoded) {
                    (BasicType::Integer, BasicType::Integer) => {
                        let plan = if type_rhs.is_scalar {
                            if type_lhs.is_encoded() {
                                let encoded = QueryPlan::EncodeIntConstant(Box::new(plan_rhs), type_lhs.codec.unwrap());
                                QueryPlan::LessThanVS(type_lhs.encoding_type(), Box::new(plan_lhs), Box::new(encoded))
                            } else {
                                QueryPlan::LessThanVS(type_lhs.encoding_type(), Box::new(plan_lhs), Box::new(plan_rhs))
                            }
                        } else {
                            unimplemented!()
                        };
                        (plan, Type::new(BasicType::Boolean, None).mutable())
                    }
                    _ => panic!("type error: {:?} < {:?}", type_lhs, type_rhs)
                }
            }
            Func(Equals, ref lhs, ref rhs) => {
                let (plan_lhs, type_lhs) = QueryPlan::create_query_plan(lhs, columns, filter.clone());
                let (plan_rhs, type_rhs) = QueryPlan::create_query_plan(rhs, columns, filter);
                match (type_lhs.decoded, type_rhs.decoded) {
                    (BasicType::String, BasicType::String) => {
                        let plan = if type_rhs.is_scalar {
                            if type_lhs.is_encoded() {
                                let encoded = QueryPlan::EncodeStrConstant(Box::new(plan_rhs), type_lhs.codec.unwrap());
                                QueryPlan::EqualsVS(type_lhs.encoding_type(), Box::new(plan_lhs), Box::new(encoded))
                            } else {
                                QueryPlan::EqualsVS(type_lhs.encoding_type(), Box::new(plan_lhs), Box::new(plan_rhs))
                            }
                        } else {
                            unimplemented!()
                        };
                        (plan, Type::new(BasicType::Boolean, None).mutable())
                    }
                    (BasicType::Integer, BasicType::Integer) => {
                         let plan = if type_rhs.is_scalar {
                            if type_lhs.is_encoded() {
                                let encoded = QueryPlan::EncodeIntConstant(Box::new(plan_rhs), type_lhs.codec.unwrap());
                                QueryPlan::EqualsVS(type_lhs.encoding_type(), Box::new(plan_lhs), Box::new(encoded))
                            } else {
                                QueryPlan::EqualsVS(type_lhs.encoding_type(), Box::new(plan_lhs), Box::new(plan_rhs))
                            }
                        } else {
                            unimplemented!()
                        };
                        (plan, Type::new(BasicType::Boolean, None).mutable())
                    }
                    _ => panic!("type error: {:?} = {:?}", type_lhs, type_rhs)
                }
            }
            Func(Or, ref lhs, ref rhs) => {
                let (plan_lhs, type_lhs) = QueryPlan::create_query_plan(lhs, columns, filter.clone());
                let (plan_rhs, type_rhs) = QueryPlan::create_query_plan(rhs, columns, filter);
                assert!(type_lhs.decoded == BasicType::Boolean && type_rhs.decoded == BasicType::Boolean);
                (QueryPlan::Or(Box::new(plan_lhs), Box::new(plan_rhs)), Type::bit_vec())
            }
            Func(And, ref lhs, ref rhs) => {
                let (plan_lhs, type_lhs) = QueryPlan::create_query_plan(lhs, columns, filter.clone());
                let (plan_rhs, type_rhs) = QueryPlan::create_query_plan(rhs, columns, filter);
                assert!(type_lhs.decoded == BasicType::Boolean && type_rhs.decoded == BasicType::Boolean);
                (QueryPlan::And(Box::new(plan_lhs), Box::new(plan_rhs)), Type::bit_vec())
            }
            Const(ref v) => (QueryPlan::Constant(v.clone()), Type::scalar(v.get_type())),
            ref x => panic!("{:?}.compile_vec() not implemented", x),
        }
    }

    pub fn compile_grouping_key<'b>(exprs: &[Expr],
                                    columns: &HashMap<&'b str, &'b Column>,
                                    filter: Filter) -> (QueryPlan<'b>, Type<'b>) {
        assert!(exprs.len() == 1);
        QueryPlan::create_query_plan(&exprs[0], columns, filter)
    }
}

