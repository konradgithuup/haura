import pandas as pd
import seaborn as sns
import matplotlib.pyplot as plt

# Load
df = pd.read_csv('tuning_results.csv')

# Pivot (Cooling vs Spread). Mean ops/s for ratio intersections.
pivot = df.pivot_table(
    index='cooling',
    columns='spread',
    values='ops_per_sec',
    aggfunc='mean'
)

# Plot
plt.figure(figsize=(10, 8))
sns.heatmap(pivot, annot=True, fmt=".1f", cmap="viridis")
plt.title('WHATT Parameter Tuning: Mean Ops/s')
plt.ylabel('Cooling Factor')
plt.xlabel('Mutation Spread')

plt.tight_layout()
plt.savefig('whatt_tuning.png')
print("Plot saved to whatt_tuning.png")
