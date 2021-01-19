# Behaviours

## Performers

If you need an abstract `Address` of `Actor`s that can receive actions
you should declare own trait that covers all necessay functions and
implement it for all suitable addresses.

## Bridges

If you need a bridge between different runtimes it's recommened to use
common sockets and protocols that using workarounds of the framework.

## Supervisor

You should prefer to have `Supervisor` actor that catches termination signals and
creates other actors, because it the supoervisor will call caonstructors of others
it can provider all necesary information and actors will have addresses of other parts
in constructors and you don't have to wrap that fields with `Option`.
