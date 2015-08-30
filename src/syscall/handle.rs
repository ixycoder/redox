use alloc::boxed::*;

use core::cmp::max;
use core::cmp::min;
use core::ptr;

use common::context::*;
use common::event::*;
use common::pio::*;
use common::resource::*;
use common::scheduler::*;

use graphics::color::*;
use graphics::window::*;

use syscall::common::*;

pub unsafe fn syscall_handle(eax: u32, ebx: u32, ecx: u32, edx: u32){
    match eax {
        SYS_DEBUG => { //Debug
            if ::debug_display as usize > 0 {
                let display = &*(*::debug_display);
                if ebx == 10 {
                    ::debug_point.x = 0;
                    ::debug_point.y += 16;
                    ::debug_redraw = true;
                }else{
                    display.char(::debug_point, (ebx as u8) as char, Color::new(255, 255, 255));
                    ::debug_point.x += 8;
                }
                if ::debug_point.x >= display.width as isize {
                    ::debug_point.x = 0;
                    ::debug_point.y += 16;
                }
                while ::debug_point.y + 16 > display.height as isize {
                    display.scroll(16);
                    ::debug_point.y -= 16;
                }
            }

            outb(0x3F8, ebx as u8);
        },
        SYS_EXIT => {
            let reenable = start_no_ints();

            let contexts = &mut *(*contexts_ptr);

            if contexts.len() > 1 && context_i > 1 {
                let current_option = contexts.remove(context_i);

                if context_i >= contexts.len() {
                    context_i -= contexts.len();
                }
                match current_option {
                    Option::Some(mut current) => match contexts.get(context_i) {
                        Option::Some(next) => {
                            current.remap(next);
                            current.switch(next);
                        },
                        Option::None => ()
                    },
                    Option::None => ()
                }
            }

            end_no_ints(reenable);
        },
        SYS_OPEN => {
            let session = &mut *::session_ptr;
            let url_ptr = ebx as *const URL;
            ptr::write(ecx as *mut Box<Resource>, session.open(&*url_ptr));
        },
        SYS_TRIGGER => {
            let mut event = *(ebx as *const Event);

            if event.code == 'm' {
                event.a = max(0, min((*::session_ptr).display.width as isize - 1, (*::session_ptr).mouse_point.x + event.a));
                event.b = max(0, min((*::session_ptr).display.height as isize - 1, (*::session_ptr).mouse_point.y + event.b));
                (*::session_ptr).mouse_point.x = event.a;
                (*::session_ptr).mouse_point.y = event.b;
                (*::session_ptr).redraw = max((*::session_ptr).redraw, REDRAW_CURSOR);
            }
            if event.code == 'k' && event.b == 0x3B && event.c > 0 {
                ::debug_draw = true;
                ::debug_redraw = true;
            }
            if event.code == 'k' && event.b == 0x3C && event.c > 0 {
                ::debug_draw = false;
                (*::session_ptr).redraw = max((*::session_ptr).redraw, REDRAW_ALL);
            }

            let reenable = start_no_ints();

            //TODO: Dispatch to appropriate window
            (*::events_ptr).push(event);

            end_no_ints(reenable);
        },
        SYS_WINDOW_CREATE => {
            let reenable = start_no_ints();

            (*::session_ptr).windows.insert(0, ebx as *mut Window);

            end_no_ints(reenable);
        },
        SYS_WINDOW_DESTROY => {
            let reenable = start_no_ints();

            let mut i = 0;
            while i < (*::session_ptr).windows.len() {
                let mut remove = false;

                match (*::session_ptr).windows.get(i) {
                    Option::Some(window_ptr) => if *window_ptr == ebx as *mut Window {
                        remove = true;
                    }else{
                        i += 1;
                    },
                    Option::None => break
                }

                if remove {
                    (*::session_ptr).windows.remove(i);
                }
            }

            end_no_ints(reenable);
        },
        SYS_YIELD => {
            let reenable = start_no_ints();

            let contexts = &*(*contexts_ptr);
            let current_i = context_i;
            context_i += 1;
            if context_i >= contexts.len(){
                context_i -= contexts.len();
            }
            if context_i != current_i {
                match contexts.get(current_i){
                    Option::Some(current) => match contexts.get(context_i) {
                        Option::Some(next) => {
                            current.remap(next);
                            current.switch(next);
                        },
                        Option::None => ()
                    },
                    Option::None => ()
                }
            }

            end_no_ints(reenable);
        },
        _ => ()
    }
}
