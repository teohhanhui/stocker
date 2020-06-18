use reactive_rs::{Broadcast, Stream};

pub trait StreamExt<'a>: Stream<'a> {
    fn combine_latest<U, F, T>(self, other: U, func: F) -> CombineLatest<Self, U, NoContext<F>>
    where
        U: Stream<'a>,
        F: 'a + FnMut(&(Self::Item, U::Item)) -> T,
        Self::Item: 'a + Clone + Sized,
        U::Item: 'a + Clone + Sized,
    {
        CombineLatest {
            func: NoContext(func),
            stream_a: self,
            stream_b: other,
        }
    }
}

impl<'a, T> StreamExt<'a> for T where T: Stream<'a> {}

pub struct CombineLatest<S, U, F> {
    func: F,
    stream_a: S,
    stream_b: U,
}

impl<'a, S, U, F, A, B, C> Stream<'a> for CombineLatest<S, U, F>
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
        let mut func = self.func;
        let sink = Broadcast::new();
        sink.clone()
            .fold_ctx(
                (None, None, None),
                |ctx: &Option<C>, (acc_ctx, acc_a, acc_b), (a, b): &(Option<A>, Option<B>)| {
                    (
                        if ctx.is_some() {
                            ctx.clone()
                        } else {
                            acc_ctx.clone()
                        },
                        if a.is_some() {
                            a.clone()
                        } else {
                            acc_a.clone()
                        },
                        if b.is_some() {
                            b.clone()
                        } else {
                            acc_b.clone()
                        },
                    )
                },
            )
            .map_both(|(ctx, a, b)| (ctx.clone(), (a.clone(), b.clone())))
            .filter_ctx(|ctx, (a, b)| ctx.is_some() && a.is_some() && b.is_some())
            .map_both_ctx(|ctx, (a, b)| {
                (
                    ctx.clone().unwrap(),
                    (a.clone().unwrap(), b.clone().unwrap()),
                )
            })
            .map_ctx(move |ctx, x| func.call_mut(ctx, x))
            .subscribe_ctx(move |ctx, x| {
                observer(ctx, x);
            });
        {
            let sink = sink.clone();
            self.stream_a.subscribe_ctx(move |ctx, a| {
                sink.send_ctx(Some(ctx.clone()), (Some(a.clone()), None));
            });
        }
        {
            let sink = sink.clone();
            self.stream_b.subscribe(move |b| {
                sink.send((None, Some(b.clone())));
            });
        }
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
