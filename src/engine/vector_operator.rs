use std::marker::PhantomData;
use bit_vec::BitVec;
use std::rc::Rc;
use ingest::raw_val::RawVal;
use mem_store::column::{ColumnData, ColumnCodec};
use engine::typed_vec::TypedVec;
use engine::query::QueryStats;


pub type BoxedOperator<'a> = Box<VecOperator<'a> + 'a>;

pub trait VecOperator<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a>;
}


pub struct GetDecode<'a> { col: &'a ColumnData }

impl<'a> GetDecode<'a> {
    pub fn new(col: &'a ColumnData) -> GetDecode { GetDecode { col: col } }
}

impl<'a> VecOperator<'a> for GetDecode<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        stats.start();
        let result = self.col.collect_decoded();
        stats.record(&"decode");
        stats.ops += result.len();
        result
    }
}

pub struct FilterDecode<'a> {
    col: &'a ColumnData,
    filter: Rc<BitVec>,
}

impl<'a> FilterDecode<'a> {
    pub fn new(col: &'a ColumnData, filter: Rc<BitVec>) -> FilterDecode<'a> {
        FilterDecode {
            col: col,
            filter: filter,
        }
    }
}

impl<'a> VecOperator<'a> for FilterDecode<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        stats.start();
        let result = self.col.filter_decode(self.filter.as_ref());
        stats.record(&"filter_decode");
        stats.ops += self.filter.len();
        result
    }
}

pub struct IndexDecode<'a> {
    col: &'a ColumnData,
    filter: Rc<Vec<usize>>,
}

impl<'a> IndexDecode<'a> {
    pub fn new(col: &'a ColumnData, filter: Rc<Vec<usize>>) -> IndexDecode<'a> {
        IndexDecode {
            col: col,
            filter: filter,
        }
    }
}

impl<'a> VecOperator<'a> for IndexDecode<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        stats.start();
        let result = self.col.index_decode(self.filter.as_ref());
        stats.record(&"index_decode");
        stats.ops += self.filter.len();
        result
    }
}


pub struct GetEncoded<'a> { col: &'a ColumnCodec }

impl<'a> GetEncoded<'a> {
    pub fn new(col: &'a ColumnCodec) -> GetEncoded { GetEncoded { col: col } }
}

impl<'a> VecOperator<'a> for GetEncoded<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        stats.start();
        let result = self.col.get_encoded();
        stats.record(&"get_encoded");
        result
    }
}


pub struct FilterEncoded<'a> {
    col: &'a ColumnCodec,
    filter: Rc<BitVec>,
}

impl<'a> FilterEncoded<'a> {
    pub fn new(col: &'a ColumnCodec, filter: Rc<BitVec>) -> FilterEncoded<'a> {
        FilterEncoded {
            col: col,
            filter: filter,
        }
    }
}

impl<'a> VecOperator<'a> for FilterEncoded<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        stats.start();
        let result = self.col.filter_encoded(self.filter.as_ref());
        stats.record(&"filter_encoded");
        stats.ops += self.filter.len();
        result
    }
}

pub struct IndexEncoded<'a> {
    col: &'a ColumnCodec,
    filter: Rc<Vec<usize>>,
}

impl<'a> IndexEncoded<'a> {
    pub fn new(col: &'a ColumnCodec, filter: Rc<Vec<usize>>) -> IndexEncoded<'a> {
        IndexEncoded {
            col: col,
            filter: filter,
        }
    }
}

impl<'a> VecOperator<'a> for IndexEncoded<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        stats.start();
        let result = self.col.index_encoded(self.filter.as_ref());
        stats.record(&"index_encoded");
        stats.ops += self.filter.len();
        result
    }
}

pub struct Decode<'a> { plan: BoxedOperator<'a> }

impl<'a> Decode<'a> {
    pub fn new(plan: BoxedOperator<'a>) -> Decode<'a> {
        Decode { plan: plan }
    }
}

impl<'a> VecOperator<'a> for Decode<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let encoded = self.plan.execute(stats);
        stats.start();
        let result = encoded.decode();
        stats.record(&"decode");
        result
    }
}


pub struct Constant { val: RawVal }

impl Constant {
    pub fn new(val: RawVal) -> Constant {
        Constant { val: val }
    }
}

impl<'a> VecOperator<'a> for Constant {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'static> {
        stats.start();
        let result = TypedVec::Constant(self.val.clone());
        stats.record(&"constant");
        result
    }
}


pub struct LessThanVSi64<'a> {
    lhs: BoxedOperator<'a>,
    rhs: i64,
}

impl<'a> LessThanVSi64<'a> {
    pub fn new(lhs: BoxedOperator, rhs: i64) -> LessThanVSi64 {
        LessThanVSi64 {
            lhs: lhs,
            rhs: rhs,
        }
    }
}

impl<'a> VecOperator<'a> for LessThanVSi64<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let lhs = self.lhs.execute(stats);
        stats.start();
        let data = lhs.cast_ref_i64();
        let mut result = BitVec::with_capacity(lhs.len());
        let i = self.rhs;
        for l in data {
            result.push(*l < i);
        }
        stats.record(&"less_than_vsi_64");
        stats.ops += data.len();
        TypedVec::Boolean(result)
    }
}


pub struct LessThanVSu8<'a> {
    lhs: BoxedOperator<'a>,
    rhs: u8,
}

impl<'a> LessThanVSu8<'a> {
    pub fn new(lhs: BoxedOperator, rhs: u8) -> LessThanVSu8 {
        LessThanVSu8 {
            lhs: lhs,
            rhs: rhs,
        }
    }
}

impl<'a> VecOperator<'a> for LessThanVSu8<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let lhs = self.lhs.execute(stats);
        stats.start();
        let data = lhs.cast_ref_u8().0;
        let mut result = BitVec::with_capacity(data.len());
        let i = self.rhs;
        for l in data {
            result.push(*l < i);
        }
        stats.record(&"less_than_vs_u8");
        stats.ops += data.len();
        TypedVec::Boolean(result)
    }
}


pub struct EqualsVSString<'a> {
    lhs: BoxedOperator<'a>,
    rhs: BoxedOperator<'a>,
}

impl<'a> EqualsVSString<'a> {
    pub fn new(lhs: BoxedOperator<'a>, rhs: BoxedOperator<'a>) -> EqualsVSString<'a> {
        EqualsVSString {
            lhs: lhs,
            rhs: rhs,
        }
    }
}

impl<'a> VecOperator<'a> for EqualsVSString<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let lhs = self.lhs.execute(stats);
        let rhs = self.rhs.execute(stats);
        stats.start();
        let data = lhs.cast_ref_str();
        let s = rhs.cast_str_const();
        let mut result = BitVec::with_capacity(data.len());
        for l in data {
            result.push(*l == s);
        }
        stats.record(&"equals_vs_str");
        stats.ops += data.len();
        TypedVec::Boolean(result)
    }
}


pub struct EqualsVSU16<'a> {
    lhs: BoxedOperator<'a>,
    rhs: BoxedOperator<'a>,
}

impl<'a> EqualsVSU16<'a> {
    pub fn new(lhs: BoxedOperator<'a>, rhs: BoxedOperator<'a>) -> EqualsVSU16<'a> {
        EqualsVSU16 {
            lhs: lhs,
            rhs: rhs,
        }
    }
}

impl<'a> VecOperator<'a> for EqualsVSU16<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let lhs = self.lhs.execute(stats);
        let rhs = self.rhs.execute(stats);
        stats.start();
        let data = lhs.cast_ref_u16().0;
        // TODO(clemens): range check
        let s = rhs.cast_int_const() as u16;
        let mut result = BitVec::with_capacity(data.len());
        for l in data {
            result.push(*l == s);
        }
        stats.record(&"equals_vs_u16");
        stats.ops += data.len();
        TypedVec::Boolean(result)
    }
}


struct BooleanOperator<'a, T> {
    lhs: BoxedOperator<'a>,
    rhs: BoxedOperator<'a>,
    op: PhantomData<T>,
}

impl<'a, T: BooleanOp + 'a> BooleanOperator<'a, T> {
    fn new(lhs: BoxedOperator<'a>, rhs: BoxedOperator<'a>) -> BoxedOperator<'a> {
        Box::new(BooleanOperator::<'a, T> {
            lhs: lhs,
            rhs: rhs,
            op: PhantomData,
        })
    }
}

pub struct Boolean;

impl Boolean {
    pub fn or<'a>(lhs: BoxedOperator<'a>, rhs: BoxedOperator<'a>) -> BoxedOperator<'a> {
        BooleanOperator::<BooleanOr>::new(lhs, rhs)
    }

    pub fn and<'a>(lhs: BoxedOperator<'a>, rhs: BoxedOperator<'a>) -> BoxedOperator<'a> {
        BooleanOperator::<BooleanAnd>::new(lhs, rhs)
    }
}

impl<'a, T: BooleanOp> VecOperator<'a> for BooleanOperator<'a, T> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let _lhs = self.lhs.execute(stats);
        let _rhs = self.rhs.execute(stats);

        stats.start();
        let mut result = _lhs.cast_bit_vec();
        let rhs = _rhs.cast_bit_vec();
        T::evaluate(&mut result, &rhs);
        stats.record(T::name());

        TypedVec::Boolean(result)
    }
}

trait BooleanOp {
    fn evaluate(lhs: &mut BitVec, rhs: &BitVec);
    fn name() -> &'static str;
}

struct BooleanOr;

struct BooleanAnd;

impl BooleanOp for BooleanOr {
    fn evaluate(lhs: &mut BitVec, rhs: &BitVec) { lhs.union(rhs); }
    fn name() -> &'static str { &"bit_vec_or" }
}

impl BooleanOp for BooleanAnd {
    fn evaluate(lhs: &mut BitVec, rhs: &BitVec) { lhs.intersect(rhs); }
    fn name() -> &'static str { &"bit_vec_and" }
}


pub struct EncodeStrConstant<'a> {
    constant: BoxedOperator<'a>,
    codec: &'a ColumnCodec,
}

impl<'a> EncodeStrConstant<'a> {
    pub fn new(constant: BoxedOperator<'a>, codec: &'a ColumnCodec) -> EncodeStrConstant<'a> {
        EncodeStrConstant {
            constant: constant,
            codec: codec,
        }
    }
}

impl<'a> VecOperator<'a> for EncodeStrConstant<'a> {
    fn execute(&mut self, stats: &mut QueryStats) -> TypedVec<'a> {
        let constant = self.constant.execute(stats);

        stats.start();
        let s = constant.cast_str_const();
        let result = self.codec.encode_str(s);
        stats.record(&"encode_str_const");

        TypedVec::Constant(result)
    }
}