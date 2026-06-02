import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useCreateRun, getListRunsQueryKey, getGetResearchSummaryQueryKey } from "@workspace/api-client-react";
import { Layout } from "@/components/layout";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { useLocation } from "wouter";
import { useQueryClient } from "@tanstack/react-query";
import { useToast } from "@/hooks/use-toast";
import { Save } from "lucide-react";

const formSchema = z.object({
  symbol: z.string().min(1, "Symbol is required").toUpperCase(),
  strategy: z.string().min(1, "Strategy is required"),
  startDate: z.string().optional(),
  endDate: z.string().optional(),
  notes: z.string().optional(),
});

export default function NewRun() {
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const createMutation = useCreateRun();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      symbol: "",
      strategy: "",
      startDate: "",
      endDate: "",
      notes: "",
    },
  });

  function onSubmit(values: z.infer<typeof formSchema>) {
    createMutation.mutate({
      data: {
        symbol: values.symbol,
        strategy: values.strategy,
        startDate: values.startDate || undefined,
        endDate: values.endDate || undefined,
        notes: values.notes || undefined,
      }
    }, {
      onSuccess: (run) => {
        toast({ title: "Run created successfully" });
        queryClient.invalidateQueries({ queryKey: getListRunsQueryKey() });
        queryClient.invalidateQueries({ queryKey: getGetResearchSummaryQueryKey() });
        setLocation(`/runs/${run.id}`);
      },
      onError: () => {
        toast({ title: "Failed to create run", variant: "destructive" });
      }
    });
  }

  return (
    <Layout>
      <div className="max-w-2xl mx-auto space-y-8">
        <div>
          <h1 className="text-2xl font-mono tracking-tight font-bold">New Backtest Run</h1>
          <p className="text-sm text-muted-foreground">Record a new strategy backtest simulation.</p>
        </div>

        <div className="border border-border bg-card p-6">
          <Form {...form}>
            <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-6">
              <div className="grid grid-cols-2 gap-6">
                <FormField
                  control={form.control}
                  name="symbol"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="font-mono text-xs uppercase text-muted-foreground">Symbol</FormLabel>
                      <FormControl>
                        <Input placeholder="BTC-USD" className="font-mono rounded-none border-border bg-background focus-visible:ring-primary" {...field} />
                      </FormControl>
                      <FormMessage className="font-mono text-xs text-destructive" />
                    </FormItem>
                  )}
                />
                <FormField
                  control={form.control}
                  name="strategy"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="font-mono text-xs uppercase text-muted-foreground">Strategy Name</FormLabel>
                      <FormControl>
                        <Input placeholder="Mean Reversion V2" className="font-mono rounded-none border-border bg-background focus-visible:ring-primary" {...field} />
                      </FormControl>
                      <FormMessage className="font-mono text-xs text-destructive" />
                    </FormItem>
                  )}
                />
              </div>

              <div className="grid grid-cols-2 gap-6">
                <FormField
                  control={form.control}
                  name="startDate"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="font-mono text-xs uppercase text-muted-foreground">Start Date</FormLabel>
                      <FormControl>
                        <Input type="date" className="font-mono rounded-none border-border bg-background focus-visible:ring-primary" {...field} />
                      </FormControl>
                      <FormMessage className="font-mono text-xs text-destructive" />
                    </FormItem>
                  )}
                />
                <FormField
                  control={form.control}
                  name="endDate"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="font-mono text-xs uppercase text-muted-foreground">End Date</FormLabel>
                      <FormControl>
                        <Input type="date" className="font-mono rounded-none border-border bg-background focus-visible:ring-primary" {...field} />
                      </FormControl>
                      <FormMessage className="font-mono text-xs text-destructive" />
                    </FormItem>
                  )}
                />
              </div>

              <FormField
                control={form.control}
                name="notes"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel className="font-mono text-xs uppercase text-muted-foreground">Research Notes</FormLabel>
                    <FormControl>
                      <Textarea 
                        placeholder="Parameters used, market conditions, observations..." 
                        className="font-mono rounded-none border-border bg-background focus-visible:ring-primary min-h-[150px]" 
                        {...field} 
                      />
                    </FormControl>
                    <FormMessage className="font-mono text-xs text-destructive" />
                  </FormItem>
                )}
              />

              <div className="flex justify-end pt-4 border-t border-border">
                <Button 
                  type="submit" 
                  className="font-mono rounded-none bg-primary text-primary-foreground hover:bg-primary/90"
                  disabled={createMutation.isPending}
                >
                  <Save size={16} className="mr-2" />
                  {createMutation.isPending ? "INITIALIZING..." : "INITIATE RUN"}
                </Button>
              </div>
            </form>
          </Form>
        </div>
      </div>
    </Layout>
  );
}
