## Dependencies

* [rust](https://rustup.rs/)
* [cargo make](https://github.com/sagiegurari/cargo-make)
  * `cargo install cargo-make`
* [clang](https://clang.llvm.org/)
  * on Windows I used `scoop` to install clang and then set the enviroment variable: `LIBCLANG_PATH` to `C:\Users\xnor\scoop\apps\llvm\current\bin`

## Examples

checkout the examples in **median/examples**
You can build/package/install them with:

`cargo make build`
`cargo make package`
`cargo make install`

## Resources
- [writing docs in rust](https://facility9.com/2016/05/writing-documentation-in-rust/)

josuha
```
if you have more than 7 args or if you are mixing floating point and other argument types, use A_GIMME
```

# TODO

* m1
* cross compile
  * osxcross
  * mingw
  * ```
    rustup target add x86_64-pc-windows-gnu
    rustup toolchain install stable-x86_64-pc-windows-gnu
    cargo build --target x86_64-pc-windows-gnu
    ```

* `#S` for clock object

```c
t_scheduler *sched = scheduler_fromobject((t_object *) mainobject);
object_obex_storeflags(child, gensym("#S"), (t_object *) sched, OBJ_FLAG_DATA);

t_patcher *p=NULL;
t_box *b=NULL;
t_max_err err;
err = object_obex_lookup(x, gensym("#P"), (t_object **)&p);
err = object_obex_lookup(x, gensym("#B"), (t_object **)&b);
```
