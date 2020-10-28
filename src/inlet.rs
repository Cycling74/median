pub type FloatCB<T> = Box<dyn Fn(&T, f64)>;
pub type IntCB<T> = Box<dyn Fn(&T, i64)>;

pub enum MaxInlet<T> {
    Float(FloatCB<T>),
    Int(IntCB<T>),
    Proxy,
}

pub enum MSPInlet<T> {
    Float(FloatCB<T>),
    Int(IntCB<T>),
    Proxy,
    Signal,
}
