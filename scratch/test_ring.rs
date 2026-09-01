use ringbuf::HeapRb;
fn main() {
    let rb = HeapRb::<i32>::new(10);
    let (mut prod, mut cons) = rb.split();
}
