# Behaviours

## Performers

If you need an abstract `Address` of `Actor`s that can receive actions
you should declare own trait that covers all necessay functions and
implement it for all suitable addresses.

## Bridges

If you need a bridge between different runtimes it's recommened to use
common sockets and protocols that using workarounds of the framework.
