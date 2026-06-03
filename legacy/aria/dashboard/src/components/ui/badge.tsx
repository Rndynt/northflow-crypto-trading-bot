import { cn } from "@/lib/utils";
import { cva, type VariantProps } from "class-variance-authority";

const badgeVariants = cva(
  "inline-flex items-center rounded-md px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide transition-colors",
  {
    variants: {
      variant: {
        default:     "bg-primary/15 text-primary",
        secondary:   "bg-secondary text-secondary-foreground",
        destructive: "bg-destructive/15 text-destructive",
        outline:     "ring-1 ring-border text-foreground",
        profit:      "bg-profit/15 text-profit",
        loss:        "bg-loss/15 text-loss",
        warning:     "bg-warning/15 text-warning",
        info:        "bg-info/15 text-info",
        muted:       "bg-muted text-muted-foreground",
      },
    },
    defaultVariants: { variant: "default" },
  }
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
