use derivative::Derivative;
use im::{hashmap, HashMap};
use reactive_rs::{Broadcast, Stream};
use std::{cell::RefCell, hash::Hash, rc::Rc};

pub trait StreamExt<'a>: Stream<'a> {
    fn buffer(self, count: usize) -> Buffer<Self, Self::Item>
    where
        Self::Item: 'a + Clone + Sized,
    {
        Buffer {
            buf: Rc::new(RefCell::new(Vec::with_capacity(count))),
            count,
            stream: self,
        }
    }

    fn combine_latest<U, F, T>(
        self,
        other: U,
        func: F,
    ) -> CombineLatest<Self, U, NoContext<F>, Self::Item, U::Item, Self::Context>
    where
        U: Stream<'a>,
        F: 'a + FnMut(&(Self::Item, U::Item)) -> T,
        Self::Item: 'a + Clone + Sized,
        Self::Context: 'a + Clone + Sized,
        U::Item: 'a + Clone + Sized,
    {
        CombineLatest {
            buf_a: Rc::new(RefCell::new(None)),
            buf_b: Rc::new(RefCell::new(None)),
            buf_ctx: Rc::new(RefCell::new(None)),
            func: NoContext(func),
            stream_a: self,
            stream_b: other,
        }
    }

    fn distinct_until_changed(self) -> DistinctUntilChanged<Self, Self::Item>
    where
        Self::Item: 'a + Clone + PartialEq + Sized,
    {
        DistinctUntilChanged {
            buf: Rc::new(RefCell::new(None)),
            stream: self,
        }
    }

    fn group_by<F, G, K, V>(
        self,
        key_func: F,
        value_func: G,
    ) -> GroupBy<'a, Self, NoContext<F>, NoContext<G>, K, V, Self::Context>
    where
        F: 'a + FnMut(&Self::Item) -> K,
        G: 'a + FnMut(&Self::Item) -> V,
        Self::Item: 'a + Clone + Sized,
        Self::Context: 'a + Sized,
    {
        GroupBy {
            key_func: NoContext(key_func),
            key_grouped_map: Rc::new(RefCell::new(hashmap! {})),
            stream: self,
            value_func: NoContext(value_func),
        }
    }

    fn merge<U>(self, other: U) -> Merge<Self, U>
    where
        U: Stream<'a, Item = Self::Item, Context = Self::Context>,
    {
        Merge {
            stream_a: self,
            stream_b: other,
        }
    }

    fn switch(self) -> Switch<Self>
    where
        Self::Item: Stream<'a>,
    {
        Switch { stream: self }
    }

    fn with_latest_from<U, F, T>(
        self,
        other: U,
        func: F,
    ) -> WithLatestFrom<Self, U, NoContext<F>, U::Item>
    where
        U: Stream<'a>,
        F: 'a + FnMut(&(Self::Item, U::Item)) -> T,
        Self::Item: 'a + Clone + Sized,
        U::Item: 'a + Clone + Sized,
    {
        WithLatestFrom {
            buf_b: Rc::new(RefCell::new(None)),
            func: NoContext(func),
            stream_a: self,
            stream_b: other,
        }
    }
}

impl<'a, T> StreamExt<'a> for T where T: Stream<'a> {}

pub struct Buffer<S, T: Sized> {
    buf: Rc<RefCell<Vec<T>>>,
    count: usize,
    stream: S,
}

impl<'a, S, T> Stream<'a> for Buffer<S, T>
where
    S: Stream<'a, Item = T>,
    T: 'a + Clone + Sized,
{
    type Context = S::Context;
    type Item = [T];

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        self.stream.subscribe_ctx({
            let buf = self.buf;
            let count = self.count;
            move |ctx, x| {
                let full = {
                    buf.borrow_mut().push(x.clone());
                    buf.borrow().len() == count
                };
                if full {
                    observer(ctx, &buf.borrow()[..]);
                    buf.borrow_mut().clear();
                }
            }
        });
    }
}

pub struct CombineLatest<S, U, F, A: Sized, B: Sized, C: Sized> {
    buf_a: Rc<RefCell<Option<A>>>,
    buf_b: Rc<RefCell<Option<B>>>,
    buf_ctx: Rc<RefCell<Option<C>>>,
    func: F,
    stream_a: S,
    stream_b: U,
}

impl<'a, S, U, F, A, B, C> Stream<'a> for CombineLatest<S, U, F, A, B, C>
where
    S: Stream<'a, Item = A, Context = C>,
    U: Stream<'a, Item = B>,
    F: 'a + ContextFn<C, (A, B)>,
    A: 'a + Clone + Sized,
    B: 'a + Clone + Sized,
    C: 'a + Clone + Sized,
{
    type Context = C;
    type Item = F::Output;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        let sink: Broadcast<C, (A, B)> = Broadcast::new();
        sink.clone().subscribe_ctx({
            let mut func = self.func;
            move |ctx, x| {
                observer(ctx, &func.call_mut(ctx, x));
            }
        });
        self.stream_a.subscribe_ctx({
            let buf_a = self.buf_a.clone();
            let buf_b = self.buf_b.clone();
            let buf_ctx = self.buf_ctx.clone();
            let sink = sink.clone();
            move |ctx, a| {
                buf_a.borrow_mut().replace(a.clone());
                buf_ctx.borrow_mut().replace(ctx.clone());
                let buf_b = buf_b.borrow();
                if let Some(b) = buf_b.as_ref() {
                    let b = b.clone();
                    drop(buf_b);
                    sink.send_ctx(ctx, &(a.clone(), b));
                }
            }
        });
        self.stream_b.subscribe({
            let buf_a = self.buf_a;
            let buf_b = self.buf_b;
            let buf_ctx = self.buf_ctx;
            move |b| {
                buf_b.borrow_mut().replace(b.clone());
                let buf_a = buf_a.borrow();
                if let Some(a) = buf_a.as_ref() {
                    let a = a.clone();
                    let buf_ctx = buf_ctx.borrow();
                    let ctx = buf_ctx.as_ref().cloned().unwrap();
                    drop(buf_a);
                    drop(buf_ctx);
                    sink.send_ctx(&ctx, &(a, b.clone()));
                }
            }
        });
    }
}

pub struct DistinctUntilChanged<S, T: Sized> {
    buf: Rc<RefCell<Option<T>>>,
    stream: S,
}

impl<'a, S, T> Stream<'a> for DistinctUntilChanged<S, T>
where
    S: Stream<'a, Item = T>,
    T: 'a + Clone + PartialEq + Sized,
{
    type Context = S::Context;
    type Item = T;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        self.stream.subscribe_ctx({
            let buf = self.buf;
            move |ctx, x| {
                if !matches!(&*buf.borrow(), Some(y) if x == y) {
                    buf.borrow_mut().replace(x.clone());
                    observer(ctx, x);
                }
            }
        });
    }
}

pub struct GroupBy<'a, S, F, G, K: Sized, V: Sized, C> {
    key_func: F,
    #[allow(clippy::type_complexity)]
    key_grouped_map: Rc<RefCell<HashMap<K, Grouped<'a, K, V, C>>>>,
    stream: S,
    value_func: G,
}

impl<'a, S, F, G, K, V, T, C> Stream<'a> for GroupBy<'a, S, F, G, K, V, C>
where
    S: Stream<'a, Item = T, Context = C>,
    F: 'a + ContextFn<C, T, Output = K>,
    G: 'a + ContextFn<C, T, Output = V>,
    K: 'a + Clone + Eq + Hash,
    V: 'a,
    C: 'a,
{
    type Context = C;
    type Item = Grouped<'a, K, V, C>;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        self.stream.subscribe_ctx({
            let mut key_func = self.key_func;
            let key_grouped_map = self.key_grouped_map;
            let mut value_func = self.value_func;
            move |ctx, x| {
                let key = key_func.call_mut(ctx, x);
                let mut key_grouped_map = key_grouped_map.borrow_mut();
                let grouped = key_grouped_map.entry(key.clone()).or_insert_with(|| {
                    let grouped = Grouped {
                        key: key.clone(),
                        sink: Broadcast::new(),
                    };
                    observer(ctx, &grouped);
                    grouped
                });
                grouped.sink.send_ctx(ctx, value_func.call_mut(ctx, x));
            }
        });
    }
}

#[derive(Derivative)]
#[derivative(Clone(bound = "K: Clone"))]
pub struct Grouped<'a, K: Sized, V: 'a + Sized, C: 'a> {
    pub key: K,
    sink: Broadcast<'a, C, V>,
}

impl<'a, K, V, C> Stream<'a> for Grouped<'a, K, V, C>
where
    V: 'a,
    C: 'a,
{
    type Context = C;
    type Item = V;

    fn subscribe_ctx<O>(self, observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        self.sink.subscribe_ctx(observer);
    }
}

pub struct Merge<S, U> {
    stream_a: S,
    stream_b: U,
}

impl<'a, S, U, T, C> Stream<'a> for Merge<S, U>
where
    S: Stream<'a, Item = T, Context = C>,
    U: Stream<'a, Item = T, Context = C>,
    T: 'a,
    C: 'a,
{
    type Context = C;
    type Item = T;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        let sink = Broadcast::new();
        sink.clone().subscribe_ctx(move |ctx, x| {
            observer(ctx, x);
        });
        self.stream_a.subscribe_ctx({
            let sink = sink.clone();
            move |ctx, x| {
                sink.send_ctx(ctx, x);
            }
        });
        self.stream_b.subscribe_ctx(move |ctx, x| {
            sink.send_ctx(ctx, x);
        });
    }
}

pub struct Switch<S> {
    stream: S,
}

impl<'a, S, X, T, C> Stream<'a> for Switch<S>
where
    S: Stream<'a, Item = X>,
    X: Clone + Stream<'a, Item = T, Context = C>,
    T: 'a,
    C: 'a,
{
    type Context = C;
    type Item = T;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        let sink = Broadcast::new();
        sink.clone().subscribe_ctx(move |ctx, x| {
            observer(ctx, x);
        });
        self.stream.subscribe(move |inner_stream| {
            inner_stream.clone().subscribe_ctx({
                let sink = sink.clone();
                move |ctx, x| {
                    sink.send_ctx(ctx, x);
                }
            });
        });
    }
}

pub struct WithLatestFrom<S, U, F, B: Sized> {
    buf_b: Rc<RefCell<Option<B>>>,
    func: F,
    stream_a: S,
    stream_b: U,
}

impl<'a, S, U, F, A, B> Stream<'a> for WithLatestFrom<S, U, F, B>
where
    S: Stream<'a, Item = A>,
    U: Stream<'a, Item = B>,
    F: 'a + ContextFn<S::Context, (A, B)>,
    A: 'a + Clone + Sized,
    B: 'a + Clone + Sized,
{
    type Context = S::Context;
    type Item = F::Output;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        self.stream_a.subscribe_ctx({
            let buf_b = self.buf_b.clone();
            let mut func = self.func;
            move |ctx, a| {
                let buf_b = buf_b.borrow();
                if let Some(b) = buf_b.as_ref() {
                    let b = b.clone();
                    drop(buf_b);
                    observer(ctx, &func.call_mut(ctx, &(a.clone(), b)));
                }
            }
        });
        self.stream_b.subscribe({
            let buf_b = self.buf_b;
            move |b| {
                buf_b.borrow_mut().replace(b.clone());
            }
        });
    }
}

pub trait ContextFn<C: ?Sized, T: ?Sized> {
    type Output;

    fn call_mut(&mut self, ctx: &C, item: &T) -> Self::Output;
}

impl<C: ?Sized, T: ?Sized, V, F> ContextFn<C, T> for F
where
    F: FnMut(&C, &T) -> V,
{
    type Output = V;

    #[inline(always)]
    fn call_mut(&mut self, ctx: &C, item: &T) -> Self::Output {
        self(ctx, item)
    }
}

pub struct NoContext<F>(F);

impl<F, C: ?Sized, T: ?Sized, V> ContextFn<C, T> for NoContext<F>
where
    F: FnMut(&T) -> V,
{
    type Output = V;

    #[inline(always)]
    fn call_mut(&mut self, _ctx: &C, item: &T) -> Self::Output {
        (self.0)(item)
    }
}
