use super::downcast::downcast;
use super::error::RuntimeErr;
use super::traits::{
    Arithmetic, DataType, PineFrom, PineRef, PineStaticType, PineType, SecondType,
};
use std::cmp::{Ordering, PartialEq, PartialOrd};
use std::convert::{From, Into};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::mem;

#[derive(Debug, Clone)]
pub struct Series<'a, D: Clone + Debug + 'a> {
    pub current: D,
    history: Vec<D>,
    phantom: PhantomData<&'a D>,
    na_val: D,
}

impl<'a, D: Default + Clone + Debug + 'a> From<D> for Series<'a, D> {
    fn from(input: D) -> Self {
        Series {
            current: input,
            history: vec![],
            phantom: PhantomData,
            na_val: D::default(),
        }
    }
}

impl<'a, D: Clone + Debug + 'a> Into<Vec<D>> for Series<'a, D> {
    fn into(self) -> Vec<D> {
        self.history
    }
}

impl<'a, D: Default + PineType<'a> + Clone + Debug + 'a> Series<'a, D> {
    pub fn new() -> Series<'a, D> {
        Series {
            current: D::default(),
            history: vec![],
            phantom: PhantomData,
            na_val: D::default(),
        }
    }

    pub fn from_vec(history: Vec<D>) -> Series<'a, D> {
        Series {
            current: D::default(),
            history,
            phantom: PhantomData,
            na_val: D::default(),
        }
    }

    pub fn index(&self, i: usize) -> Result<Series<'a, D>, RuntimeErr> {
        let len = self.history.len();
        let val = match i {
            // m if m < 0 => Err(SeriesErr::Negative),
            0 => self.current.clone(),
            m if m >= 1 && m <= len => self.history[(len - i) as usize].clone(),
            _ => self.na_val.clone(),
        };
        Ok(Series::from(val))
    }

    pub fn update(&mut self, current: D) {
        self.current = current;
    }

    pub fn commit(&mut self) {
        self.history
            .push(mem::replace(&mut self.current, D::default()));
    }

    pub fn roll_back(&mut self) {
        self.history.pop().unwrap();
    }
}

impl<'a, D: PineStaticType + Clone + Debug + 'a> PineStaticType for Series<'a, D> {
    fn static_type() -> (DataType, SecondType) {
        (<D as PineStaticType>::static_type().0, SecondType::Series)
    }
}

impl<'a, D: PineStaticType + PineType<'a> + Clone + Debug + 'a> PineType<'a> for Series<'a, D> {
    fn get_type(&self) -> (DataType, SecondType) {
        (<D as PineStaticType>::static_type().0, SecondType::Series)
    }

    fn copy(&self) -> PineRef<'a> {
        PineRef::Box(Box::new(self.clone()))
    }
}

impl<'a, D: Default + PineStaticType + Clone + Debug + 'a> PineFrom<'a, Series<'a, D>>
    for Series<'a, D>
{
    fn explicity_from(t: Box<dyn PineType<'a> + 'a>) -> Result<Box<Series<'a, D>>, RuntimeErr> {
        Self::implicity_from(t)
    }

    fn implicity_from(t: Box<dyn PineType<'a> + 'a>) -> Result<Box<Series<'a, D>>, RuntimeErr> {
        let data_type = <D as PineStaticType>::static_type().0;
        match t.get_type() {
            (d, SecondType::Series) if data_type == d => Ok(downcast::<Series<D>>(t).unwrap()),
            (d, SecondType::Simple) if data_type == d => Ok(Box::new(Series {
                current: *downcast::<D>(t).unwrap(),
                history: vec![],
                phantom: PhantomData,
                na_val: D::default(),
            })),
            _ => Err(RuntimeErr::NotCompatible),
        }
    }
}

impl<'a, D: Clone + Arithmetic + Debug + 'a> Arithmetic for Series<'a, D> {
    fn add(mut self, other: Self) -> Self {
        self.current = self.current.add(other.current);
        self
    }

    fn minus(mut self, other: Self) -> Self {
        self.current = self.current.minus(other.current);
        self
    }

    fn mul(mut self, other: Self) -> Self {
        self.current = self.current.mul(other.current);
        self
    }

    fn div(mut self, other: Self) -> Self {
        self.current = self.current.div(other.current);
        self
    }

    fn rem(mut self, other: Self) -> Self {
        self.current = self.current.rem(other.current);
        self
    }
}

impl<'a, D: PartialOrd + Clone + Debug + 'a> PartialOrd for Series<'a, D> {
    fn partial_cmp(&self, other: &Series<'a, D>) -> Option<Ordering> {
        self.current.partial_cmp(&other.current)
    }
}

impl<'a, D: PartialEq + Clone + Debug + 'a> PartialEq for Series<'a, D> {
    fn eq(&self, other: &Self) -> bool {
        self.current.eq(&other.current)
    }
}

#[cfg(test)]
mod tests {
    use super::super::primitive::Int;
    use super::*;

    #[test]
    fn series_test() {
        let int: Int = Some(1);
        let mut series = <Series<Int> as From<Int>>::from(int);
        assert_eq!(series.index(0), Ok(Series::from(Some(1))));
        series.update(Some(2));
        assert_eq!(series.index(0), Ok(Series::from(Some(2))));

        series.commit();
        assert_eq!(series.history, vec![Some(2)]);
        assert_eq!(series.current, None);

        series.roll_back();
        assert_eq!(series.history, vec![]);
    }
}
