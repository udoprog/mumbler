macro_rules! __into_target {
    ($event:expr, $target_ty:ty) => {{
        let Some(target) = $event.target() else {
            return Ok(Default::default());
        };

        let Ok(target) = target.dyn_into::<$target_ty>() else {
            return Ok(Default::default());
        };

        target
    }};
}

pub(super) use __into_target as into_target;
