proto throws {
    op throw(&self, error: any);
}

struct try<TValue> {
    op throw(&self, error: any) {
        self.error = error;
        do self.try
    }

    fn[throws] try(&self, cont: &Continuation) {
        if else_fn != null {
            return OpAction.Return(else_fn());
        } else {
            throw self.error;
        }
    }
}

proto yield<T> {
    op yield<T>(value: T);
}

proto Iterator<E, T> {
    fn[E] next(&self) -> T;
}

proto InitialHandler<TReturn> {
    fn[E] _(initial: &Initial) -> TReturn;
}

fn iterate<E: super yield<T>, T>(initial: &Initial<E, T>) -> Iterator<E, T> {

}

struct iterate<TValue> {
    struct YieldIterator<E>(cont: &Continuation<unit>) {
        fn[E] next(&self) -> TValue {
            cont.resume();
        }
    }

    op yield(&self, value: TValue) {
        self.value = value;
        do self
    }

    fn handle(&self, cont: &Continuation<unit>) -> Iterator<E, TValue> {
        self.value
    )
}

fn[yield<int>] generator() {
    for i in  {
        yield i;
    }
}

fn test2() {
    for i in std.iterate generator() {
        print(i);
    }
}

enum SomeErrors {
    NumberIsNot42,
}

fn[throws] copy(fs1: fs, fs2: fs, path1: string, path2: string) {
    if path1 == path2 {
        throw "Source and destination cannot be the same";
    }

    let f = fs1.read(path1);
    fs2.write(f, path2);
}

fn[throws] something_throws() -> int {
    throw SomeErrors.NumberIsNot42;
}


fn[throws] test() {
    let a = try something_throws();
    let b = try something_throws() else 42;
    let c = try struct(1, 2.5, "test");
    let d = try something_throws() match {
        SomeErrors.NumberIsNot42 => 42,
    };


}
