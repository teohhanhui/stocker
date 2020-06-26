use reactive_rs::{Broadcast, Stream};
use std::{cell::RefCell, rc::Rc};

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
        U::Item: 'a + Clone + Sized,
        Self::Context: 'a + Clone + Sized,
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
        let buf = self.buf.clone();
        let count = self.count;
        self.stream.subscribe_ctx(move |ctx, x| {
            let full = {
                let mut buf = buf.borrow_mut();
                buf.push(x.clone());
                buf.len() == count
            };
            if full {
                observer(ctx, &buf.borrow()[..]);
                let mut buf = buf.borrow_mut();
                buf.clear();
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
        let buf_a = self.buf_a.clone();
        let buf_b = self.buf_b.clone();
        let buf_ctx = self.buf_ctx.clone();
        let mut func = self.func;
        let sink: Broadcast<C, (A, B)> = Broadcast::new();
        sink.clone().subscribe_ctx(move |ctx, x| {
            observer(ctx, &func.call_mut(ctx, x));
        });
        self.stream_a.subscribe_ctx({
            let buf_a = buf_a.clone();
            let buf_b = buf_b.clone();
            let buf_ctx = buf_ctx.clone();
            let sink = sink.clone();
            move |ctx, a| {
                if let Some(b) = &*buf_b.borrow() {
                    sink.send_ctx(ctx, &(a.clone(), b.clone()));
                }
                let mut buf_a = buf_a.borrow_mut();
                buf_a.replace(a.clone());
                let mut buf_ctx = buf_ctx.borrow_mut();
                buf_ctx.replace(ctx.clone());
            }
        });
        self.stream_b.subscribe(move |b| {
            if let Some(a) = &*buf_a.borrow() {
                let buf_ctx = &*buf_ctx.borrow();
                let ctx = buf_ctx.as_ref().unwrap();
                sink.send_ctx(ctx, &(a.clone(), b.clone()));
            }
            let mut buf_b = buf_b.borrow_mut();
            buf_b.replace(b.clone());
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
        let buf = self.buf.clone();
        self.stream.subscribe_ctx(move |ctx, x| {
            if !matches!(&*buf.borrow(), Some(y) if x == y) {
                observer(ctx, x);
                let mut buf = buf.borrow_mut();
                buf.replace(x.clone());
            }
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
        let buf_b = self.buf_b.clone();
        let mut func = self.func;
        self.stream_a.subscribe_ctx({
            let buf_b = buf_b.clone();
            move |ctx, a| {
                if let Some(b) = {
                    let buf_b = buf_b.borrow();
                    buf_b.as_ref().cloned()
                } {
                    observer(ctx, &func.call_mut(ctx, &(a.clone(), b)));
                }
            }
        });
        self.stream_b.subscribe(move |b| {
            let mut buf_b = buf_b.borrow_mut();
            buf_b.replace(b.clone());
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
