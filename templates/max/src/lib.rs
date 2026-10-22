use median::prelude::*;

//`#[derive(MaxObject)]` makes the struct a Max class; `ext_main!` at the bottom registers it.
#[derive(MaxObject)]
#[max(name = "{{crate_name}}")]
pub struct MaxExtern {
    obj: ObjRef,
    //an attribute "foo" backed by this field (see `median::attr::AttrField` for the types)
    #[attr]
    foo: Float64,
}

//implement the max object wrapper
impl MaxObjWrapped for MaxExtern {
    //create an instance of your object
    //setup inlets/outlets and clocks
    fn new(builder: &mut MaxBuilder<Self>) -> Self {
        Self {
            obj: builder.obj(),
            foo: Float64::new(0.0),
        }
    }
}

//`#[median::methods]` registers the marked methods with Max
#[median::methods]
impl MaxExtern {
    //a "bang" method. The name of the method can be anything you want
    #[bang]
    pub fn bang(&self) {
        let i = self.obj.proxy_inlet();
        median::object_post!(&self.obj, "bang inlet {} foo {}", i, self.foo);
    }

    //an "int" method: takes 1 argument other than &self, of type t_atom_long
    #[int]
    pub fn int(&self, v: t_atom_long) {
        let i = self.obj.proxy_inlet();
        post!("int {} inlet {}", v, i);
    }
}

median::ext_main! { MaxExtern }
