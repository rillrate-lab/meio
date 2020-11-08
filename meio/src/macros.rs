/// Creates a message for `Link` to interact with an `Actor`.
#[macro_export]
macro_rules! link_interaction {
    ($msg:ident : $link:ident :: $func:ident ( $($arg:ident : $arg_t:ty),* ) -> $res:ty) => {
        pub(super) struct $msg {
            $(
                pub $arg: $arg_t,
            )*
        }

        impl meio::Interaction for $msg {
            type Output = $res;
        }

        impl $link {
            pub async fn $func(&mut self, $($arg: $arg_t,)*) -> Result<$res, anyhow::Error> {
                let msg = $msg { $($arg,)* };
                meio::InteractionPerformer::interact(&mut self.address, msg).await
            }
        }
    };
}
